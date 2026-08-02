#[derive(Debug)]
pub enum GitObject {
    Blob(Vec<u8>),
    Tree(Vec<TreeEntry>),
    Commit(CommitData),
}

#[derive(Debug)]
pub struct TreeEntry {
    pub mode: String,
    pub name: String,
    pub hash: [u8; 20],
}

#[derive(Debug)]
pub struct CommitData {
    pub tree_hash: String,
    pub parent_hashes: Vec<String>,
    pub author: String,
    pub message: String,
}