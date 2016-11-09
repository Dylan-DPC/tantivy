use combine::*;
use super::query_ast::*;

pub fn query_language(input: State<&str>) -> ParseResult<QueryAST, &str>
{
    let literal = || {
        let term_val = || {
            let word = many1(satisfy(|c: char| c.is_alphanumeric()));
            let phrase =
                (char('"'), many1(satisfy(|c| c != '"')), char('"'),)
                .map(|(_, s, _)| s);
            phrase.or(word)
        };
        
        let field = many1(letter());
        let term_query = (field, char(':'), term_val())
            .map(|(field_name,_, phrase)| {
                QueryLiteral {
                    field_name: Some(field_name),
                    phrase: phrase
                }
            });
        let term_default_field = term_val()
            .map(|phrase| {
                QueryLiteral {
                    field_name: None,
                    phrase: phrase
                }
            });

        try(term_query).or(term_default_field)
            .map(|query_literal| QueryNode::from(query_literal)) 
    };

    let leaf = || {
            (char('-'), literal()).map(|(_, expr)| QueryNode::Not(box expr))
        .or((char('+'), literal()).map(|(_, expr)| QueryNode::Must(box expr)))
        .or(literal())
    };
    
    let expr = || {
        sep_by(leaf(), spaces())
        .map(|subqueries: Vec<QueryNode>| {
            if subqueries.len() == 1 {
                subqueries.into_iter().next().unwrap()
            }
            else {
                QueryNode::Clause(subqueries.into_iter().map(Box::new).collect())
            }
        })
    };

    (expr(), eof())
        .map(|(root, _)| QueryAST::from(root))
        .parse_state(input)
}



#[cfg(test)]
mod test {
    
    use combine::*;
    use super::*;
    
    fn test_parse_query_to_ast_helper(query: &str, expected: &str) {
        let mut grammar_parser = parser(query_language);
        let query = grammar_parser.parse(query).unwrap().0;
        let query_str = format!("{:?}", query);
        assert_eq!(query_str, expected);
    }

    #[test]
    pub fn test_parse_query_to_ast() {
        test_parse_query_to_ast_helper("abc:toto", "abc:\"toto\"");
        test_parse_query_to_ast_helper("+abc:toto", "+(abc:\"toto\")");
        test_parse_query_to_ast_helper("+abc:toto -titi", "+(abc:\"toto\") -(\"titi\")");
        test_parse_query_to_ast_helper("-abc:toto", "-(abc:\"toto\")");
        test_parse_query_to_ast_helper("abc:a b", "abc:\"a\" \"b\"");
        test_parse_query_to_ast_helper("abc:\"a b\"", "abc:\"a b\"");
    }
}