use std::{fs::File, io};

/// The in-memory editable version of the database, loaded on startup, written
/// back to disk at the end.
pub struct Database {}

impl Database {
    pub fn new(_db_file: Option<File>) -> io::Result<Database> {
        todo!("initializing the database for editing");
    }

    pub fn write_into(self, _db_file: File) -> io::Result<()> {
        todo!("writing the database into a file");
    }
}
