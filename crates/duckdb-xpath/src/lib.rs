mod scalar;

use duckdb::{duckdb_entrypoint_c_api, Connection, Result};
use std::error::Error;

#[duckdb_entrypoint_c_api()]
pub unsafe fn extension_entrypoint(con: Connection) -> Result<(), Box<dyn Error>> {
    con.register_table_function::<scalar::XPathTextVTab>("xpath_text")
        .expect("Failed to register xpath_text");
    Ok(())
}
