/// Usage text of `stripspace`, printed for `-h` and usage errors.
pub const STRIPSPACE_USAGE: &str = "usage: rgit stripspace [-s | --strip-comments]\n   or: rgit stripspace [-c | --comment-lines]\n\n    -s, --strip-comments  skip and remove all lines starting with comment character\n    -c, --comment-lines   prepend comment character and space to each line\n\n";

/// True for the bytes git's cleanup treats as whitespace: space, tab, CR and LF (not form feed).
// Used by `strip_space`.
fn is_git_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// git's message cleanup on raw bytes: trailing whitespace is cut from every line, runs of empty
/// lines collapse into one, empty lines at the start and end go away, and the last line gets a
/// newline. With `strip_comments`, lines starting with `comment_char` are dropped first.
// Used by `stripspace`.
pub fn strip_space(input: &[u8], strip_comments: bool, comment_char: u8) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut pending_blank = false;
    for line in input.split_inclusive(|b| *b == b'\n') {
        if strip_comments && line.first() == Some(&comment_char) {
            continue;
        }
        let mut end = line.len();
        while end > 0 && is_git_space(line[end - 1]) {
            end -= 1;
        }
        if end == 0 {
            pending_blank = true;
            continue;
        }
        if pending_blank && !out.is_empty() {
            out.push(b'\n');
        }
        pending_blank = false;
        out.extend_from_slice(&line[..end]);
        out.push(b'\n');
    }
    out
}

/// Turns every line into a comment: `<char> ` in front, or just `<char>` when the line is empty
/// or starts with a tab. Line contents are kept as they are; the last line gets a newline.
// Used by `stripspace -c`.
pub fn comment_lines(input: &[u8], comment_char: u8) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(input.len() + input.len() / 8);
    for line in input.split_inclusive(|b| *b == b'\n') {
        out.push(comment_char);
        if !matches!(line.first(), Some(b'\n') | Some(b'\t')) {
            out.push(b' ');
        }
        out.extend_from_slice(line);
        if line.last() != Some(&b'\n') {
            out.push(b'\n');
        }
    }
    out
}
