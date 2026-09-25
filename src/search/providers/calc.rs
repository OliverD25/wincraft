use crate::search::{
    Action, Choice, Completion, Context, Glyph, IconRef, Query, ResultItem, SearchProvider,
};

/// Arithmetic typed after `=`. Enter copies the answer.
pub struct Calculator;

impl SearchProvider for Calculator {
    fn id(&self) -> &'static str {
        "calc"
    }

    fn name(&self) -> &'static str {
        "Calculator"
    }

    fn description(&self) -> &'static str {
        "Work out + \u{2212} \u{d7} \u{f7} ^ % and brackets; Enter copies the answer"
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some("=")
    }

    fn placeholder(&self) -> &'static str {
        "Type a sum, like (12 + 30) * 1.5"
    }

    fn needle<'a>(&self, _text: &'a str) -> &'a str {
        ""
    }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        let text = query.text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        let row = |title: String, subtitle: String| ResultItem {
            group: "Calculator".to_string(),
            title,
            subtitle,
            icon: IconRef::Glyph(Glyph::Calculator),
            ..Default::default()
        };
        match evaluate(text) {
            Ok(value) => {
                let answer = format_number(value);
                let mut item = row(answer.clone(), text.to_string());
                item.enter = Some(Choice {
                    label: "Copy".to_string(),
                    action: Action::Copy(answer.clone()),
                });
                item.tab = Some(Completion {
                    label: "Keep going".to_string(),
                    text: answer,
                });
                vec![item]
            }
            Err(problem) => vec![row(problem, text.to_string())],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Times,
    Divide,
    Power,
    Percent,
    Open,
    Close,
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let mut found = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let token = match c {
            ' ' | '\t' | '\u{a0}' => {
                at += 1;
                continue;
            }
            '0'..='9' | '.' | ',' => {
                let start = at;
                while at < chars.len() && matches!(chars[at], '0'..='9' | '.' | ',') {
                    at += 1;
                }
                // A comma is the decimal mark in much of Europe, and nothing
                // here takes a list of arguments, so it cannot mean anything else.
                let digits: String = chars[start..at]
                    .iter()
                    .map(|c| if *c == ',' { '.' } else { *c })
                    .collect();
                let value = digits
                    .parse::<f64>()
                    .map_err(|_| format!("{digits} is not a number"))?;
                found.push(Token::Number(value));
                continue;
            }
            '+' => Token::Plus,
            '-' | '\u{2212}' => Token::Minus,
            '*' | 'x' | 'X' | '\u{d7}' => Token::Times,
            '/' | '\u{f7}' | ':' => Token::Divide,
            '^' => Token::Power,
            '%' => Token::Percent,
            '(' => Token::Open,
            ')' => Token::Close,
            other => return Err(format!("{other} is not something I can calculate")),
        };
        found.push(token);
        at += 1;
    }
    Ok(found)
}

/// Recursive descent over the usual precedence: plus and minus below times,
/// divide and remainder, below a leading minus, below ^, which groups to the
/// right.
/// `%` after a number is a percentage unless a number follows it, in which
/// case it is the remainder: "15%" is 0.15, "10 % 3" is 1.
struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

const INCOMPLETE: &str = "Keep typing\u{2026}";

impl Parser {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.at).copied()
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.peek();
        self.at += 1;
        token
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut value = self.product()?;
        while let Some(token @ (Token::Plus | Token::Minus)) = self.peek() {
            self.at += 1;
            let right = self.product()?;
            value = if token == Token::Plus {
                value + right
            } else {
                value - right
            };
        }
        Ok(value)
    }

    fn product(&mut self) -> Result<f64, String> {
        let mut value = self.unary()?;
        loop {
            match self.peek() {
                Some(Token::Times) => {
                    self.at += 1;
                    value *= self.unary()?;
                }
                Some(Token::Divide) => {
                    self.at += 1;
                    let right = self.unary()?;
                    if right == 0.0 {
                        return Err("Cannot divide by zero".to_string());
                    }
                    value /= right;
                }
                Some(Token::Percent) if self.starts_operand(self.at + 1) => {
                    self.at += 1;
                    let right = self.unary()?;
                    if right == 0.0 {
                        return Err("Cannot divide by zero".to_string());
                    }
                    value %= right;
                }
                _ => return Ok(value),
            }
        }
    }

    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(Token::Minus) => {
                self.at += 1;
                Ok(-self.unary()?)
            }
            Some(Token::Plus) => {
                self.at += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<f64, String> {
        let base = self.percent()?;
        if self.peek() == Some(Token::Power) {
            self.at += 1;
            let exponent = self.unary()?;
            return Ok(base.powf(exponent));
        }
        Ok(base)
    }

    fn percent(&mut self) -> Result<f64, String> {
        let mut value = self.primary()?;
        while self.peek() == Some(Token::Percent) && !self.starts_operand(self.at + 1) {
            self.at += 1;
            value /= 100.0;
        }
        Ok(value)
    }

    fn primary(&mut self) -> Result<f64, String> {
        match self.next() {
            Some(Token::Number(value)) => Ok(value),
            Some(Token::Open) => {
                let value = self.sum()?;
                match self.next() {
                    Some(Token::Close) => Ok(value),
                    None => Err(INCOMPLETE.to_string()),
                    Some(_) => Err("A bracket is missing".to_string()),
                }
            }
            None => Err(INCOMPLETE.to_string()),
            Some(_) => Err("Something is missing between the signs".to_string()),
        }
    }

    fn starts_operand(&self, index: usize) -> bool {
        matches!(self.tokens.get(index), Some(Token::Number(_) | Token::Open))
    }
}

pub fn evaluate(text: &str) -> Result<f64, String> {
    let tokens = tokens(text)?;
    if tokens.is_empty() {
        return Err(INCOMPLETE.to_string());
    }
    let mut parser = Parser { tokens, at: 0 };
    let value = parser.sum()?;
    if parser.at < parser.tokens.len() {
        return Err(if parser.peek() == Some(Token::Close) {
            "A bracket closes that was never opened".to_string()
        } else {
            "Something is missing between the numbers".to_string()
        });
    }
    if !value.is_finite() {
        return Err("The answer is too large".to_string());
    }
    Ok(value)
}

/// At most ten decimals, without trailing zeros, so 0.1 + 0.2 reads 0.3.
pub fn format_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.abs() >= 1e15 || value.abs() < 1e-10 {
        return format!("{value:e}");
    }
    let text = format!("{value:.10}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(text: &str) -> String {
        match evaluate(text) {
            Ok(value) => format_number(value),
            Err(problem) => problem,
        }
    }

    #[test]
    fn the_usual_precedence_holds() {
        assert_eq!(calc("2 + 3 * 4"), "14");
        assert_eq!(calc("(2 + 3) * 4"), "20");
        assert_eq!(calc("10 - 4 - 3"), "3");
        assert_eq!(calc("100 / 10 / 5"), "2");
    }

    #[test]
    fn powers_group_to_the_right_and_bind_tighter_than_a_leading_minus() {
        assert_eq!(calc("2 ^ 3 ^ 2"), "512");
        assert_eq!(calc("-2 ^ 2"), "-4");
        assert_eq!(calc("(-2) ^ 2"), "4");
        assert_eq!(calc("2 ^ -1"), "0.5");
    }

    #[test]
    fn percent_is_a_percentage_or_a_remainder() {
        assert_eq!(calc("15%"), "0.15");
        assert_eq!(calc("200 * 15%"), "30");
        assert_eq!(calc("10 % 3"), "1");
        assert_eq!(calc("50% + 1"), "1.5");
    }

    #[test]
    fn decimals_take_a_point_or_a_comma_and_print_cleanly() {
        assert_eq!(calc("0.1 + 0.2"), "0.3");
        assert_eq!(calc("1,5 * 2"), "3");
        assert_eq!(calc("1 / 3"), "0.3333333333");
    }

    #[test]
    fn typographic_signs_work_too() {
        assert_eq!(calc("6 \u{d7} 7"), "42");
        assert_eq!(calc("84 \u{f7} 2"), "42");
        assert_eq!(calc("50 \u{2212} 8"), "42");
    }

    #[test]
    fn unfinished_or_wrong_input_says_what_is_wrong() {
        assert_eq!(calc("2 +"), INCOMPLETE);
        assert_eq!(calc("(2 + 3"), INCOMPLETE);
        assert_eq!(calc("2 + 3)"), "A bracket closes that was never opened");
        assert_eq!(calc("1 / 0"), "Cannot divide by zero");
        assert_eq!(calc("2 $ 3"), "$ is not something I can calculate");
        assert_eq!(calc("1.2.3"), "1.2.3 is not a number");
        assert_eq!(calc("2 3"), "Something is missing between the numbers");
    }

    #[test]
    fn a_result_copies_on_enter_and_continues_on_tab() {
        let query = Query {
            text: "6*7".to_string(),
            prefix: Some("=".to_string()),
            limit: 10,
        };
        let context = crate::search::test_context();
        let found = Calculator.query(&query, &context);
        assert_eq!(found[0].title, "42");
        assert_eq!(
            found[0].enter.as_ref().map(|choice| &choice.action),
            Some(&Action::Copy("42".to_string()))
        );
        assert_eq!(
            found[0].tab.as_ref().map(|tab| tab.text.as_str()),
            Some("42")
        );
    }
}
