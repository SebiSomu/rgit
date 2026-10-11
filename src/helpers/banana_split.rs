/// Determines the grouping key for a modified path according to whether directory splitting is requested.
// Used by `banana_split`.
pub fn group_key(path: &str, by_dir: bool) -> String {
    if by_dir {
        match path.split_once('/') {
            Some((top, _)) => top.to_string(),
            None => ".".to_string(),
        }
    } else {
        path.to_string()
    }
}
