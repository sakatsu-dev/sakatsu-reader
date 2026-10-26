//! What the dictionaries said during one request, so no stage has to ask twice.
//!
//! # Why a request asks the same thing over and over
//!
//! Compounds, names, reading correction, the contraction repairs and grouping each ask the
//! dictionaries their own questions, and many of those questions are the same one: the compound
//! pass and the reading pass both look up 申し分, and grouping asks again for every span it
//! considers. Each stage is right to ask — none of them can know what another already did — so the
//! memory belongs to the request, and [`Evidence`] is the handle every stage takes.
//!
//! # At the primitive level, and only for success
//!
//! What is kept is [`Dictionary::lookup`], [`Dictionary::lookup_reading`] and
//! [`Dictionary::same_entry`], each under its own operation. Everything built on them —
//! cross-reference resolution above all, which rewrites definitions — is rebuilt per call on top,
//! so a caller that changes what it was given changes nothing anyone else is given, and the
//! reading index can never be answered by the spelling-first lookup.
//!
//! **An error is never kept.** Each call that failed was a call the next caller could retry; keeping
//! one would turn a passing fault into a failure for the rest of the request.
//!
//! The key is the dictionary's generation directory, the operation and the query in the form the
//! operation itself normalises it to. The generation is part of it so a replaced dictionary can
//! never answer from a predecessor's files, even if a cache were ever handed another slice.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::dict::index::{Dictionary, LookupHit};
use crate::dict::yomitan::Entry;
use crate::dict::DictError;

/// The dictionaries a stage reads, and the request's memory of them where it has one.
#[derive(Clone, Copy)]
pub struct Evidence<'a> {
    dictionaries: &'a [Dictionary],
    kept: Option<&'a EvidenceCache>,
}

impl std::fmt::Debug for Evidence<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Evidence")
            .field("dictionaries", &self.dictionaries.len())
            .field("kept", &self.kept.is_some())
            .finish()
    }
}

impl<'a> Evidence<'a> {
    /// Straight to the dictionaries, as every stage read them before. What the plain-slice entry
    /// points use, so a harness calling one is unaffected.
    pub fn direct(dictionaries: &'a [Dictionary]) -> Evidence<'a> {
        Evidence {
            dictionaries,
            kept: None,
        }
    }

    /// Through a request's cache.
    pub fn through(dictionaries: &'a [Dictionary], kept: &'a EvidenceCache) -> Evidence<'a> {
        Evidence {
            dictionaries,
            kept: Some(kept),
        }
    }

    pub fn dictionaries(&self) -> &'a [Dictionary] {
        self.dictionaries
    }

    pub fn is_empty(&self) -> bool {
        self.dictionaries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.dictionaries.len()
    }

    /// Each dictionary in order, as a handle that asks through this evidence.
    pub fn sources(&self) -> impl Iterator<Item = Source<'a>> + 'a {
        let kept = self.kept;
        self.dictionaries
            .iter()
            .map(move |dictionary| Source { dictionary, kept })
    }
}

/// One dictionary, asked through the request's evidence.
#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub dictionary: &'a Dictionary,
    kept: Option<&'a EvidenceCache>,
}

impl<'a> Source<'a> {
    /// One dictionary, read directly.
    pub fn direct(dictionary: &'a Dictionary) -> Source<'a> {
        Source {
            dictionary,
            kept: None,
        }
    }

    /// [`Dictionary::lookup`].
    pub fn lookup(&self, query: &str) -> Result<Vec<LookupHit>, DictError> {
        let key = || query.trim().to_string();
        self.ask(Operation::Lookup, key, || self.dictionary.lookup(query))
            .map(Kept::into_hits)
    }

    /// [`Dictionary::lookup_reading`].
    pub fn lookup_reading(&self, reading: &str) -> Result<Vec<LookupHit>, DictError> {
        let key = || crate::kana::to_hiragana(reading.trim());
        self.ask(Operation::LookupReading, key, || {
            self.dictionary.lookup_reading(reading)
        })
        .map(Kept::into_hits)
    }

    /// [`Dictionary::same_entry`].
    pub fn same_entry(&self, sequence: i64, limit: usize) -> Result<Vec<Entry>, DictError> {
        let key = || sequence.abs().to_string();
        self.ask(Operation::SameEntry { limit }, key, || {
            self.dictionary
                .same_entry(sequence, limit)
                .map(Kept::Entries)
        })
        .map(Kept::into_entries)
    }

    /// [`crate::lookup::whole_word`], with every read it makes going through this evidence.
    pub fn whole_word(&self, query: &str) -> Result<Vec<crate::lookup::WholeWord>, DictError> {
        crate::lookup::resolved_through(*self, self.lookup(query)?)
    }

    /// [`crate::lookup::whole_word_by_reading`], likewise.
    pub fn whole_word_by_reading(
        &self,
        reading: &str,
    ) -> Result<Vec<crate::lookup::WholeWord>, DictError> {
        crate::lookup::resolved_through(*self, self.lookup_reading(reading)?)
    }

    fn ask<T: Into<Kept>>(
        &self,
        operation: Operation,
        key: impl FnOnce() -> String,
        read: impl FnOnce() -> Result<T, DictError>,
    ) -> Result<Kept, DictError> {
        let Some(kept) = self.kept else {
            return read().map(Into::into);
        };
        kept.ask(self.dictionary, operation, key, || read().map(Into::into))
    }
}

/// Which primitive was asked. A key part, so the reading index is never answered from a lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Operation {
    Lookup,
    LookupReading,
    SameEntry { limit: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    generation: PathBuf,
    operation: Operation,
    query: String,
}

#[derive(Clone)]
enum Kept {
    Hits(Vec<LookupHit>),
    Entries(Vec<Entry>),
}

impl From<Vec<LookupHit>> for Kept {
    fn from(hits: Vec<LookupHit>) -> Kept {
        Kept::Hits(hits)
    }
}

impl Kept {
    fn into_hits(self) -> Vec<LookupHit> {
        match self {
            Kept::Hits(hits) => hits,
            Kept::Entries(_) => unreachable!("an operation keeps one shape"),
        }
    }

    fn into_entries(self) -> Vec<Entry> {
        match self {
            Kept::Entries(entries) => entries,
            Kept::Hits(_) => unreachable!("an operation keeps one shape"),
        }
    }
}

/// One operation's reads over one request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OperationAccount {
    pub requested: usize,
    /// Answered from the cache.
    pub remembered: usize,
    /// Read from the dictionary.
    pub executed: usize,
    pub failed: usize,
}

/// Every dictionary read one request made, by operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EvidenceAccount {
    pub lookup: OperationAccount,
    pub lookup_reading: OperationAccount,
    pub same_entry: OperationAccount,
}

impl EvidenceAccount {
    fn of(&mut self, operation: Operation) -> &mut OperationAccount {
        match operation {
            Operation::Lookup => &mut self.lookup,
            Operation::LookupReading => &mut self.lookup_reading,
            Operation::SameEntry { .. } => &mut self.same_entry,
        }
    }

    /// Reads that reached a dictionary, across operations.
    pub fn executed(&self) -> usize {
        self.lookup.executed + self.lookup_reading.executed + self.same_entry.executed
    }

    pub fn requested(&self) -> usize {
        self.lookup.requested + self.lookup_reading.requested + self.same_entry.requested
    }
}

/// One request's memory of its dictionary reads. Single-threaded, like the request.
pub struct EvidenceCache {
    remember: bool,
    kept: RefCell<HashMap<Key, Kept>>,
    account: RefCell<EvidenceAccount>,
}

impl EvidenceCache {
    /// `remember: false` keeps nothing and still counts — what the legacy mode measures with.
    pub fn new(remember: bool) -> EvidenceCache {
        EvidenceCache {
            remember,
            kept: RefCell::new(HashMap::new()),
            account: RefCell::new(EvidenceAccount::default()),
        }
    }

    pub fn account(&self) -> EvidenceAccount {
        *self.account.borrow()
    }

    fn ask(
        &self,
        dictionary: &Dictionary,
        operation: Operation,
        key: impl FnOnce() -> String,
        read: impl FnOnce() -> Result<Kept, DictError>,
    ) -> Result<Kept, DictError> {
        self.account.borrow_mut().of(operation).requested += 1;
        let key = self.remember.then(|| Key {
            generation: dictionary.root().to_path_buf(),
            operation,
            query: key(),
        });
        if let Some(key) = &key {
            // Cloned out, so the borrow ends here rather than across anything the caller does.
            let found = self.kept.borrow().get(key).cloned();
            if let Some(found) = found {
                self.account.borrow_mut().of(operation).remembered += 1;
                return Ok(found);
            }
        }
        self.account.borrow_mut().of(operation).executed += 1;
        let outcome = read();
        match (&outcome, key) {
            (Ok(found), Some(key)) => {
                self.kept.borrow_mut().insert(key, found.clone());
            }
            (Err(_), _) => self.account.borrow_mut().of(operation).failed += 1,
            (Ok(_), None) => {}
        }
        outcome
    }
}
