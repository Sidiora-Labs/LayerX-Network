use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tantivy::collector::Count;
use tantivy::directory::MmapDirectory;
use tantivy::query::TermQuery;
use tantivy::schema::{Field, IndexRecordOption, Schema, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, Searcher, Term};

use crate::config::MAX_URL_BYTES;

/// The directory under the data directory the index lives in.
pub const INDEX_DIRECTORY: &str = "index";

/// The longest title a document keeps, in bytes.
pub const MAX_TITLE_BYTES: usize = 1_024;

const WRITER_THREADS: usize = 1;
const WRITER_MEMORY_BYTES: usize = 50_000_000;

#[derive(Debug)]
pub enum IndexError {
    Io(std::io::Error),
    Tantivy(tantivy::TantivyError),
    InvalidUrl,
    TitleTooLong,
    WriterPoisoned,
}

impl IndexError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "index_io_error",
            Self::Tantivy(_) => "index_error",
            Self::InvalidUrl => "invalid_index_url",
            Self::TitleTooLong => "title_too_long",
            Self::WriterPoisoned => "index_writer_poisoned",
        }
    }
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{}: {error}", self.code()),
            Self::Tantivy(error) => write!(f, "{}: {error}", self.code()),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<std::io::Error> for IndexError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<tantivy::TantivyError> for IndexError {
    fn from(error: tantivy::TantivyError) -> Self {
        Self::Tantivy(error)
    }
}

impl From<tantivy::directory::error::OpenDirectoryError> for IndexError {
    fn from(error: tantivy::directory::error::OpenDirectoryError) -> Self {
        Self::Tantivy(error.into())
    }
}

/// The three indexed fields: `url` kept whole as the document key, `title`
/// and `body` tokenised for search. All three are stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fields {
    pub url: Field,
    pub title: Field,
    pub body: Field,
}

fn schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let url = builder.add_text_field("url", STRING | STORED);
    let title = builder.add_text_field("title", TEXT | STORED);
    let body = builder.add_text_field("body", TEXT | STORED);
    (builder.build(), Fields { url, title, body })
}

/// The node's own tantivy index under `data_dir/index`, holding at most one
/// document per URL.
pub struct WebIndex {
    directory: PathBuf,
    index: Index,
    fields: Fields,
    writer: Mutex<IndexWriter>,
    reader: IndexReader,
}

impl WebIndex {
    /// Opens the index under `data_dir/index`, creating it when absent.
    ///
    /// # Errors
    /// Returns the error creating the directory, an index whose schema is not
    /// url, title and body, and a directory another writer holds.
    pub fn open(data_dir: &Path) -> Result<Self, IndexError> {
        let directory = data_dir.join(INDEX_DIRECTORY);
        std::fs::create_dir_all(&directory)?;
        let (schema, fields) = schema();
        let index = Index::open_or_create(MmapDirectory::open(&directory)?, schema)?;
        let writer = index.writer_with_num_threads(WRITER_THREADS, WRITER_MEMORY_BYTES)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        Ok(Self {
            directory,
            index,
            fields,
            writer: Mutex::new(writer),
            reader,
        })
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    #[must_use]
    pub const fn index(&self) -> &Index {
        &self.index
    }

    #[must_use]
    pub const fn fields(&self) -> Fields {
        self.fields
    }

    /// Stages a page, replacing any document already held for its URL. The
    /// page is searchable after the next [`WebIndex::commit`].
    ///
    /// # Errors
    /// Refuses an empty or over-long URL and an over-long title.
    pub fn put(&self, url: &str, title: &str, body: &str) -> Result<(), IndexError> {
        if url.is_empty() || url.len() > MAX_URL_BYTES {
            return Err(IndexError::InvalidUrl);
        }
        if title.len() > MAX_TITLE_BYTES {
            return Err(IndexError::TitleTooLong);
        }
        let writer = self.writer.lock().map_err(|_| IndexError::WriterPoisoned)?;
        writer.delete_term(Term::from_field_text(self.fields.url, url));
        writer.add_document(doc!(
            self.fields.url => url,
            self.fields.title => title,
            self.fields.body => body,
        ))?;
        Ok(())
    }

    /// Commits every staged page and makes it visible to search.
    ///
    /// # Errors
    /// Returns the error committing or reloading the reader.
    pub fn commit(&self) -> Result<(), IndexError> {
        self.writer
            .lock()
            .map_err(|_| IndexError::WriterPoisoned)?
            .commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// A searcher over the last commit.
    #[must_use]
    pub fn searcher(&self) -> Searcher {
        self.reader.searcher()
    }

    /// The number of committed documents.
    #[must_use]
    pub fn num_docs(&self) -> u64 {
        self.searcher().num_docs()
    }

    /// The number of committed documents held for a URL: one after a page is
    /// indexed, however often it is re-crawled.
    ///
    /// # Errors
    /// Returns the error running the query.
    pub fn documents_for(&self, url: &str) -> Result<usize, IndexError> {
        let query = TermQuery::new(
            Term::from_field_text(self.fields.url, url),
            IndexRecordOption::Basic,
        );
        Ok(self.searcher().search(&query, &Count)?)
    }
}
