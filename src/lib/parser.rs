use regex::{Regex, RegexBuilder};

use {LexErrorKind, LexBuildError, LexBuildResult};

use ast::{Rule, LexAST};

pub struct LexParser {
    src: String,
    newlines: Vec<usize>,
    ast: LexAST
}

lazy_static! {
    static ref RE_NAME: Regex = {
        Regex::new(r"^[a-zA-Z_][a-zA-Z_0-9]*$").unwrap()
    };
}

impl LexParser {
    fn new(src: String) -> LexParser {
        LexParser{
            src     : src,
            newlines: vec![0],
            ast     : LexAST::new()
        }
    }

    fn mk_error(&self, k: LexErrorKind, off: usize) -> LexBuildError {
        let (line, col) = self.off_to_line_col(off);
        LexBuildError{kind: k, line: line, col: col}
    }

    fn off_to_line_col(&self, off: usize) -> (usize, usize) {
        if off == self.src.len() {
            let line_off = *self.newlines.iter().last().unwrap();
            return (self.newlines.len(), self.src[line_off..].chars().count() + 1);
        }
        let (line_m1, &line_off) = self.newlines.iter()
                                                .enumerate()
                                                .rev()
                                                .find(|&(_, &line_off)| line_off <= off)
                                                .unwrap();
        let c_off = self.src[line_off..]
                        .char_indices()
                        .position(|(c_off, _)| c_off == off - line_off)
                        .unwrap();
        return (line_m1 + 1, c_off + 1);
    }

    fn parse(&mut self) -> LexBuildResult<usize> {
        let mut i = try!(self.parse_declarations(0));
        i = try!(self.parse_rules(i));
        // We don't currently support the subroutines part of a specification. One day we might...
        match self.lookahead_is("%%", i) {
            Some(j) => {
                if try!(self.parse_ws(j)) == self.src.len() { Ok(i) }
                else {
                    Err(self.mk_error(LexErrorKind::RoutinesNotSupported, i))
                }
            }
            None    => Ok(i)
        }
    }

    fn parse_declarations(&mut self, mut i: usize) -> LexBuildResult<usize> {
        i = try!(self.parse_ws(i));
        if let Some(j) = self.lookahead_is("%%", i) { return Ok(j); }
        if i < self.src.len() {
            Err(self.mk_error(LexErrorKind::UnknownDeclaration, i))
        }
        else {
            Err(self.mk_error(LexErrorKind::PrematureEnd, i - 1))
        }
    }

    fn parse_rules(&mut self, mut i: usize) -> LexBuildResult<usize> {
        loop {
            i = try!(self.parse_ws(i));
            if i == self.src.len() { break; }
            if self.lookahead_is("%%", i).is_some() { break; }
            i = try!(self.parse_rule(i));
        }
        Ok(i)
    }

    fn parse_rule(&mut self, i: usize) -> LexBuildResult<usize> {
        let line_len = self.src[i..]
                           .find(|c| c == '\n')
                           .unwrap_or(self.src.len() - i);
        let line     = self.src[i..i + line_len].trim_right();
        let rspace   = match line.rfind(' ') {
            Some(j) => j,
            None    => return Err(self.mk_error(LexErrorKind::MissingSpace, i))
        };

        let name;
        let orig_name = &line[rspace + 1..];
        if orig_name == ";" {
            name = None;
        }
        else if self.ast.get_rule_by_name(orig_name).is_some() {
            return Err(self.mk_error(LexErrorKind::DuplicateName, i + rspace + 1))
        }
        else {
            if !RE_NAME.is_match(&orig_name) {
                return Err(self.mk_error(LexErrorKind::InvalidName, i + rspace + 1))
            }
            name = Some(orig_name.to_string());
        }

        let re_str = line[..rspace].trim_right().to_string();
        let re = match RegexBuilder::new(&format!("\\A(?:{})", &re_str))
                                    .multi_line(true)
                                    .dot_matches_new_line(true)
                                    .build() {
            Ok(x) => x,
            Err(_)  => return Err(self.mk_error(LexErrorKind::RegexError, i))
        };
        let rules_len = self.ast.rules.len();
        self.ast.set_rule(Rule{tok_id: rules_len,
                               name: name,
                               re: re,
                               re_str: re_str});
        Ok(i + line_len)
    }

    fn parse_ws(&mut self, i: usize) -> LexBuildResult<usize> {
        let mut j = i;
        for c in self.src[i..].chars() {
            match c {
                ' '  | '\t' => (),
                '\n' | '\r' => self.newlines.push(j + 1),
                _           => break
            }
            j += c.len_utf8();
        }
        Ok(j)
    }

    fn lookahead_is(&self, s: &'static str, i: usize) -> Option<usize> {
        if self.src[i..].starts_with(s) {
            Some(i + s.len())
        }
        else {
            None
        }
    }
}

pub fn parse_lex(s: &str) -> Result<LexAST, LexBuildError> {
    let mut lp = LexParser::new(s.to_string());
    match lp.parse() {
        Ok(_) => Ok(lp.ast),
        Err(e) => Err(e)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use {LexBuildError, LexErrorKind};

    #[test]
    fn test_nooptions() {
        let src = "
%option nounput
        ".to_string();
        assert!(parse_lex(&src).is_err());
    }

    #[test]
    fn test_minimum() {
        let src = "%%".to_string();
        assert!(parse_lex(&src).is_ok());
    }

    #[test]
    fn test_rules() {
        let src = "%%
[0-9]+ int
[a-zA-Z]+ id
".to_string();
        let ast = parse_lex(&src).unwrap();
        let intrule = ast.get_rule_by_name("int").unwrap();
        assert_eq!("int", intrule.name.as_ref().unwrap());
        assert_eq!("[0-9]+", intrule.re_str);
        let idrule = ast.get_rule_by_name("id").unwrap();
        assert_eq!("id", idrule.name.as_ref().unwrap());
        assert_eq!("[a-zA-Z]+", idrule.re_str);
    }

    #[test]
    fn test_no_name() {
        let src = "%%
[0-9]+ ;
".to_string();
        let ast = parse_lex(&src).unwrap();
        let intrule = ast.rules.get(0).unwrap();
        assert!(intrule.name.is_none());
        assert_eq!("[0-9]+", intrule.re_str);
    }

    #[test]
    fn test_broken_rule() {
        let src = "%%
[0-9]
int".to_string();
        assert!(parse_lex(&src).is_err());
        match parse_lex(&src) {
            Ok(_)  => panic!("Broken rule parsed"),
            Err(LexBuildError{kind: LexErrorKind::MissingSpace, line: 2, col: 1}) => (),
            Err(e) => panic!("Incorrect error returned {}", e)
        }
    }

    #[test]
    fn test_broken_rule2() {
        let src = "%%
[0-9] ".to_string();
        assert!(parse_lex(&src).is_err());
        match parse_lex(&src) {
            Ok(_)  => panic!("Broken rule parsed"),
            Err(LexBuildError{kind: LexErrorKind::MissingSpace, line: 2, col: 1}) => (),
            Err(e) => panic!("Incorrect error returned {}", e)
        }
    }

    #[test]
    fn test_invalid_name() {
        let src = "%%
[0-9] int.2".to_string();
        match parse_lex(&src) {
            Ok(_)  => panic!("Invalid name parsed"),
            Err(LexBuildError{kind: LexErrorKind::InvalidName, line: 2, col: 7}) => (),
            Err(e) => panic!("Incorrect error returned {}", e)
        }
    }

    #[test]
    fn test_duplicate_rule() {
        let src = "%%
[0-9] int
[0-9] int".to_string();
        match parse_lex(&src) {
            Ok(_)  => panic!("Duplicate rule parsed"),
            Err(LexBuildError{kind: LexErrorKind::DuplicateName, line: 3, col: 7}) => (),
            Err(e) => panic!("Incorrect error returned {}", e)
        }
    }
}
