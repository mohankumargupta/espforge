//! Parsed syntax: what the parser produces, before values are assembled.
//!
//! [`ParsedNode`] is a tag name, its id, attributes with byte spans, and the
//! node's offset. The `$` reference graph that used to live here is rewritten
//! schema-driven with the element framework; nothing in this slice needs it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `key="v"`
    Set,
    /// `key+="v"`
    Append,
}

#[derive(Debug, Clone)]
pub struct TagAttribute<'a> {
    /// Full key, possibly dotted (`network1.ssid`).
    pub key: &'a str,
    pub op: AttrOp,
    /// Byte offset of the key's first character in the original file.
    pub key_offset: usize,
    pub val: &'a str,
    /// Byte offset of the value's first character in the original file.
    pub val_offset: usize,
}

#[derive(Debug, Clone)]
pub struct ParsedNode<'a> {
    pub tag_name: &'a str,
    pub id: &'a str,
    pub attributes: Vec<TagAttribute<'a>>,
    /// Byte offset of the node's tag start in the original file.
    pub offset: usize,
}

impl<'a> ParsedNode<'a> {
    pub fn get_attr(&self, key: &str) -> Option<&'a str> {
        self.attributes.iter().find(|a| a.key == key).map(|a| a.val)
    }

    pub fn get_attr_spanned(&self, key: &str) -> Option<&TagAttribute<'a>> {
        self.attributes.iter().find(|a| a.key == key)
    }
}
