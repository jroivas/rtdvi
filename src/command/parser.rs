//! Tokenizer for the ex command line.
//!
//! v1 handles `:name arg1 arg2 [!]`. Ranges (`:1,$s/...`) are out of scope.

#[derive(Debug, Clone)]
pub struct ParsedExLine {
    pub name: String,
    pub bang: bool,
    pub args: ExArgs,
}

#[derive(Debug, Clone, Default)]
pub struct ExArgs {
    pub raw: String,
    pub words: Vec<String>,
    pub bang: bool,
}

impl ExArgs {
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.raw.is_empty()
    }
    pub fn first(&self) -> Option<&str> {
        self.words.first().map(|s| s.as_str())
    }
}

pub fn parse(line: &str) -> Result<ParsedExLine, String> {
    let line = line.trim_start_matches(':').trim_start();
    if line.is_empty() {
        return Err("empty command".into());
    }
    let mut iter = line.char_indices();
    let mut name_end = line.len();
    let mut bang = false;
    while let Some((i, c)) = iter.next() {
        if c == '!' {
            name_end = i;
            bang = true;
            // Consume the rest of `:cmd! args...`
            let after = line[i + 1..].trim_start().to_string();
            let words = split_args(&after);
            return Ok(ParsedExLine {
                name: line[..name_end].to_string(),
                bang,
                args: ExArgs { raw: after, words, bang },
            });
        }
        if c.is_whitespace() {
            name_end = i;
            break;
        }
    }
    let raw_args = line[name_end..].trim_start().to_string();
    let words = split_args(&raw_args);
    Ok(ParsedExLine {
        name: line[..name_end].to_string(),
        bang,
        args: ExArgs { raw: raw_args, words, bang },
    })
}

fn split_args(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_only() {
        let p = parse(":q").unwrap();
        assert_eq!(p.name, "q");
        assert!(!p.bang);
        assert!(p.args.is_empty());
    }

    #[test]
    fn name_with_bang() {
        let p = parse(":q!").unwrap();
        assert_eq!(p.name, "q");
        assert!(p.bang);
    }

    #[test]
    fn name_and_args() {
        let p = parse(":w foo.txt").unwrap();
        assert_eq!(p.name, "w");
        assert_eq!(p.args.first(), Some("foo.txt"));
        assert_eq!(p.args.raw, "foo.txt");
    }

    #[test]
    fn leading_colon_optional() {
        let p = parse("split").unwrap();
        assert_eq!(p.name, "split");
    }
}
