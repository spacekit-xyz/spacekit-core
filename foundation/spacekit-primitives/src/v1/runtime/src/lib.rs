/// Runtime Engine for the SWTCH Network
/// This is a runtime environment for the SWTCH Network
/// which is responsible for executing the WASM Application Modules
/// Similar to the Ethereum Virtual Machine (EVM), the Runtime Engine 
/// which runs WASM Applications for Distributed Storage and Computation.
use std::collections::HashMap;

// TODO: Implement the WASM Runtime Loader
pub struct RuntimeEngine {
// TODO: Runtime is a runtime environment for the SWTCH Network
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
}

pub struct RuntimeEngineConfig {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub gas_limit: u64,
    pub gas_price: u64,
    pub memory_limit: u64,
    pub storage_limit: u64,
}

pub struct Module {
    pub code: Vec<u8>,
    pub entry_point: String,
    pub arguments: Vec<String>,
    pub environment: HashMap<String, String>,
}

impl Module {
    pub fn new(code: Vec<u8>, entry_point: String, arguments: Vec<String>, environment: HashMap<String, String>) -> Self {
        Self { code, entry_point, arguments, environment }
    }

    pub fn from_file(file_path: String) -> Result<Self, Box<dyn std::error::Error>> {
        let code = std::fs::read(file_path)?;
        let entry_point = "main".to_string();
        let arguments = vec![];
        let environment = HashMap::new();
        Ok(Self::new(code, entry_point, arguments, environment))
    }
}

pub struct Instance {
    pub module: Module,
    pub arguments: Vec<String>,
    pub environment: HashMap<String, String>,
}

impl Instance {
    pub fn new(module: Module, arguments: Vec<String>, environment: HashMap<String, String>) -> Self {
        Self { module, arguments, environment }
    }
}

// TODO: Implement the WASM Runtime Loader and Execution Engine
impl RuntimeEngine {
    pub fn new(name: String, version: String, description: String, author: String, license: String) -> Self {
        Self { name, version, description, author, license }
    }

    pub fn load_wasm_module(module_path: String) -> Result<(), Box<dyn std::error::Error>> {
        let module = Module::from_file(module_path)?;
        Ok(())
    }

    pub fn execute_wasm_module(module: Module) -> Result<(), Box<dyn std::error::Error>> {
        let instance = Instance::new(module)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let runtime = RuntimeEngine::new("SWTCH Runtime".to_string(), "1.0.0".to_string(), "SWTCH Runtime".to_string(), "SWTCH Network".to_string(), "MIT".to_string());
        assert_eq!(runtime.name, "SWTCH Runtime".to_string());
        assert_eq!(runtime.version, "1.0.0".to_string());
        assert_eq!(runtime.description, "SWTCH Runtime".to_string());
        assert_eq!(runtime.author, "SWTCH Network".to_string());
        assert_eq!(runtime.license, "MIT".to_string());
    }
}
