use duckdb::{
    core::{DataChunkHandle, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
    Result,
};
use std::ffi::CString;
use std::sync::Mutex;

// TODO: Replace with proper scalar function when duckdb-rs supports it.
// For now, xpath_text is a table function that takes (xml VARCHAR, expr VARCHAR)
// and returns (result VARCHAR).

#[repr(C)]
pub struct XPathTextBindData {
    xml: String,
    expr: String,
}

struct XPathTextState {
    results: Vec<String>,
    offset: usize,
    initialized: bool,
}

#[repr(C)]
pub struct XPathTextInitData {
    state: Mutex<XPathTextState>,
}

pub struct XPathTextVTab;

impl VTab for XPathTextVTab {
    type InitData = XPathTextInitData;
    type BindData = XPathTextBindData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        bind.add_result_column("result", LogicalTypeHandle::from(LogicalTypeId::Varchar));

        let xml = bind.get_parameter(0).to_string();
        let expr = bind.get_parameter(1).to_string();

        Ok(XPathTextBindData { xml, expr })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(XPathTextInitData {
            state: Mutex::new(XPathTextState {
                results: Vec::new(),
                offset: 0,
                initialized: false,
            }),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let init_data = func.get_init_data();
        let bind_data = func.get_bind_data();
        let mut state = init_data.state.lock().unwrap();

        if !state.initialized {
            state.initialized = true;

            let index = simdxml::parse(bind_data.xml.as_bytes())?;
            let texts = index.xpath_text(&bind_data.expr)?;
            state.results = texts.into_iter().map(|s| s.to_string()).collect();
        }

        let remaining = state.results.len() - state.offset;
        if remaining == 0 {
            output.set_len(0);
            return Ok(());
        }

        let chunk_size = remaining.min(2048);
        let vector = output.flat_vector(0);

        for i in 0..chunk_size {
            let text = &state.results[state.offset + i];
            let c_str = CString::new(text.as_str())?;
            vector.insert(i, c_str);
        }

        output.set_len(chunk_size);
        state.offset += chunk_size;

        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![
            LogicalTypeHandle::from(LogicalTypeId::Varchar), // xml
            LogicalTypeHandle::from(LogicalTypeId::Varchar), // xpath expr
        ])
    }
}
