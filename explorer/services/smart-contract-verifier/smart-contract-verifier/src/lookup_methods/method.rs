use foundry_compilers::artifacts::sourcemap::SourceMap;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    pub selector: [u8; 4],
    pub offset: usize,
    pub length: usize,
    pub filename: String,
}

impl Method {
    pub fn from_source_map(
        selector: [u8; 4],
        source_map: &SourceMap,
        index: usize,
        file_ids: &BTreeMap<u32, String>,
    ) -> anyhow::Result<Self> {
        let src = source_map
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("source map doesn't have function index"))?;

        let filename = src
            .index()
            .and_then(|id| file_ids.get(&id))
            .ok_or_else(|| anyhow::anyhow!("src {:?} not found in output sources", src.index()))?;

        Ok(Method {
            selector,
            offset: src.offset() as usize,
            length: src.length() as usize,
            filename: filename.clone(),
        })
    }
}
