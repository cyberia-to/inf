//! Tokenizer for the inf surface (specs/grammar.md).
//!
//! `:=` is a single token; a bare `:` is the bind separator and also the lead of
//! a program option (`:limit`, `:link`, …) — the parser disambiguates by
//! position, so the lexer keeps `:` simple. `//` runs to end of line.

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Str(String),
    Assign,   // :=
    Question, // ?
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    LParen,
    RParen,
    Comma,
    Colon,
    Dot,
    Eq,
    Plus,
    Minus,
    Star,
    Hash,  // #
    At,    // @
    Tilde, // ~
    Slash, // /
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spanned {
    pub tok: Tok,
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LexError {
    pub msg: String,
    pub line: usize,
    pub col: usize,
}

pub fn lex(src: &str) -> Result<Vec<Spanned>, LexError> {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut col = 1usize;
    let mut out = Vec::new();

    let advance = |i: &mut usize, _line: &mut usize, col: &mut usize, n: usize| {
        for _ in 0..n {
            *i += 1;
            *col += 1;
        }
    };

    while i < b.len() {
        let c = b[i];
        let (sl, sc) = (line, col);
        match c {
            ' ' | '\t' | '\r' => advance(&mut i, &mut line, &mut col, 1),
            '\n' => {
                i += 1;
                line += 1;
                col = 1;
            }
            '/' if i + 1 < b.len() && b[i + 1] == '/' => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '/' => {
                out.push(Spanned { tok: Tok::Slash, line: sl, col: sc });
                advance(&mut i, &mut line, &mut col, 1);
            }
            ':' if i + 1 < b.len() && b[i + 1] == '=' => {
                out.push(Spanned { tok: Tok::Assign, line: sl, col: sc });
                advance(&mut i, &mut line, &mut col, 2);
            }
            '[' | ']' | '{' | '}' | '(' | ')' | ',' | ':' | '.' | '=' | '+' | '-' | '*'
            | '#' | '@' | '~' | '?' => {
                let t = match c {
                    '[' => Tok::LBracket,
                    ']' => Tok::RBracket,
                    '{' => Tok::LBrace,
                    '}' => Tok::RBrace,
                    '(' => Tok::LParen,
                    ')' => Tok::RParen,
                    ',' => Tok::Comma,
                    ':' => Tok::Colon,
                    '.' => Tok::Dot,
                    '=' => Tok::Eq,
                    '+' => Tok::Plus,
                    '-' => Tok::Minus,
                    '*' => Tok::Star,
                    '#' => Tok::Hash,
                    '@' => Tok::At,
                    '~' => Tok::Tilde,
                    '?' => Tok::Question,
                    _ => unreachable!(),
                };
                out.push(Spanned { tok: t, line: sl, col: sc });
                advance(&mut i, &mut line, &mut col, 1);
            }
            '"' => {
                advance(&mut i, &mut line, &mut col, 1);
                let mut s = String::new();
                while i < b.len() && b[i] != '"' {
                    if b[i] == '\\' && i + 1 < b.len() {
                        advance(&mut i, &mut line, &mut col, 1);
                        let e = b[i];
                        s.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    } else if b[i] == '\n' {
                        return Err(LexError { msg: "newline in string".into(), line, col });
                    } else {
                        s.push(b[i]);
                    }
                    advance(&mut i, &mut line, &mut col, 1);
                }
                if i >= b.len() {
                    return Err(LexError { msg: "unterminated string".into(), line: sl, col: sc });
                }
                advance(&mut i, &mut line, &mut col, 1); // closing quote
                out.push(Spanned { tok: Tok::Str(s), line: sl, col: sc });
            }
            d if d.is_ascii_digit() => {
                let mut n = String::new();
                while i < b.len() && b[i].is_ascii_digit() {
                    n.push(b[i]);
                    advance(&mut i, &mut line, &mut col, 1);
                }
                let v: i64 = n.parse().map_err(|_| LexError {
                    msg: format!("integer out of range: {n}"),
                    line: sl,
                    col: sc,
                })?;
                out.push(Spanned { tok: Tok::Int(v), line: sl, col: sc });
            }
            a if a.is_ascii_alphabetic() || a == '_' => {
                let mut id = String::new();
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
                    id.push(b[i]);
                    advance(&mut i, &mut line, &mut col, 1);
                }
                out.push(Spanned { tok: Tok::Ident(id), line: sl, col: sc });
            }
            other => {
                return Err(LexError {
                    msg: format!("unexpected character: {other:?}"),
                    line: sl,
                    col: sc,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn assign_vs_colon() {
        assert_eq!(
            toks("?[x] := a"),
            vec![
                Tok::Question,
                Tok::LBracket,
                Tok::Ident("x".into()),
                Tok::RBracket,
                Tok::Assign,
                Tok::Ident("a".into()),
            ]
        );
        // bind colon and option colon are the same token; the parser splits them
        assert_eq!(toks("to: p"), vec![Tok::Ident("to".into()), Tok::Colon, Tok::Ident("p".into())]);
        assert_eq!(toks(":limit"), vec![Tok::Colon, Tok::Ident("limit".into())]);
    }

    #[test]
    fn sigils_and_paths() {
        assert_eq!(
            toks("#cyber/truth @me ~name"),
            vec![
                Tok::Hash,
                Tok::Ident("cyber".into()),
                Tok::Slash,
                Tok::Ident("truth".into()),
                Tok::At,
                Tok::Ident("me".into()),
                Tok::Tilde,
                Tok::Ident("name".into()),
            ]
        );
    }

    #[test]
    fn comments_and_strings() {
        assert_eq!(toks("a // comment\n b"), vec![Tok::Ident("a".into()), Tok::Ident("b".into())]);
        assert_eq!(toks(r#""hello""#), vec![Tok::Str("hello".into())]);
    }

    #[test]
    fn ints_and_minus() {
        assert_eq!(toks("-3 42"), vec![Tok::Minus, Tok::Int(3), Tok::Int(42)]);
    }
}
