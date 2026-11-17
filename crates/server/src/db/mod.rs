mod repository;
mod sqlite;

pub use repository::{CharacterSaveRecord, Storage};
pub use sqlite::SqliteStorage;
