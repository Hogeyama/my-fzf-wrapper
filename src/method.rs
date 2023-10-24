use clap::Parser;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

////////////////////////////////////////////////////////////////////////////////////////////////////
// Method
////////////////////////////////////////////////////////////////////////////////////////////////////

pub trait Method {
    type Param: Serialize + DeserializeOwned;
    type Response: Serialize + DeserializeOwned;
    fn method_name() -> &'static str;
    fn request(self, params: Self::Param) -> Request;
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Request
////////////////////////////////////////////////////////////////////////////////////////////////////

/// Request sent from client to server.
#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Request {
    Load {
        method: Load,
        params: <Load as Method>::Param,
    },
    Reload {
        method: Reload,
        params: <Reload as Method>::Param,
    },
    Preview {
        method: Preview,
        params: <Preview as Method>::Param,
    },
    Run {
        method: Run,
        params: <Run as Method>::Param,
    },
    GetLastLoad {
        method: GetLastLoad,
        params: <GetLastLoad as Method>::Param,
    },
    ChangeMode {
        method: ChangeMode,
        params: <ChangeMode as Method>::Param,
    },
    ChangeDirectory {
        method: ChangeDirectory,
        params: <ChangeDirectory as Method>::Param,
    },
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Load method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Load;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct LoadParam {
    pub mode: String,
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct LoadResp {
    pub header: String,
    pub items: Vec<String>,
}

impl LoadResp {
    pub fn new_with_default_header(items: Vec<String>) -> Self {
        let pwd = std::env::current_dir().unwrap().into_os_string();
        Self {
            header: format!("[{}]", pwd.to_string_lossy()),
            items,
        }
    }
    pub fn error(err: impl ToString) -> Self {
        Self {
            header: "[error]".to_string(),
            items: vec![err.to_string()],
        }
    }
}

impl Method for Load {
    type Param = LoadParam;
    type Response = LoadResp;
    fn method_name() -> &'static str {
        "load"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Load {
            method: Load,
            params,
        }
    }
}

impl TryFrom<String> for Load {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl From<Load> for String {
    fn from(_: Load) -> Self {
        <Load as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Reload method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Reload;

impl Method for Reload {
    type Param = ();
    type Response = LoadResp;
    fn method_name() -> &'static str {
        "reload"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Reload {
            method: Reload,
            params,
        }
    }
}

impl TryFrom<String> for Reload {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl From<Reload> for String {
    fn from(_: Reload) -> Self {
        <Reload as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Preview method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Preview;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct PreviewParam {
    pub item: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct PreviewResp {
    pub message: String,
}

impl Method for Preview {
    type Param = PreviewParam;
    type Response = PreviewResp;
    fn method_name() -> &'static str {
        "preview"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Preview {
            method: Preview,
            params,
        }
    }
}

impl PreviewResp {
    pub fn error(err: impl ToString) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl TryFrom<String> for Preview {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl From<Preview> for String {
    fn from(_: Preview) -> Self {
        <Preview as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Run method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Run;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct RunParam {
    pub item: String,
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct RunResp;

// fzf の key binding で渡すオプション。
// Load と異なり、Run のオプションは共通になる。
#[derive(Parser, Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunOpts {
    #[clap(long)]
    pub tabedit: bool,

    #[clap(long)]
    pub vifm: bool,

    #[clap(long)]
    pub delete: bool,

    #[clap(long)]
    pub force: bool,

    #[clap(long)]
    pub browse_github: bool,

    #[clap(long)]
    pub menu: bool,
}

impl Method for Run {
    type Param = RunParam;
    type Response = RunResp;
    fn method_name() -> &'static str {
        "run"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Run {
            method: Run,
            params,
        }
    }
}

impl TryFrom<String> for Run {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl From<Run> for String {
    fn from(_: Run) -> Self {
        <Run as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// GetLastLoad method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct GetLastLoad;

impl Method for GetLastLoad {
    type Param = ();
    type Response = LoadResp;
    fn method_name() -> &'static str {
        "get_last_load"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::GetLastLoad {
            method: GetLastLoad,
            params,
        }
    }
}

impl TryFrom<String> for GetLastLoad {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}

impl From<GetLastLoad> for String {
    fn from(_: GetLastLoad) -> Self {
        <GetLastLoad as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// ChangeMode method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct ChangeMode;

impl Method for ChangeMode {
    type Param = ChangeModeParam;
    type Response = ();
    fn method_name() -> &'static str {
        "change_mode"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::ChangeMode {
            method: ChangeMode,
            params,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ChangeModeParam {
    pub mode: String,
    pub query: Option<String>,
    pub args: Vec<String>,
}

impl TryFrom<String> for ChangeMode {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}

impl From<ChangeMode> for String {
    fn from(_: ChangeMode) -> Self {
        <ChangeMode as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// ChangeDirectory method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct ChangeDirectory;

impl Method for ChangeDirectory {
    type Param = ChangeDirectoryParam;
    type Response = ();
    fn method_name() -> &'static str {
        "change_directory"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::ChangeDirectory {
            method: ChangeDirectory,
            params,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ChangeDirectoryParam {
    ToParent,
    ToLastFileDir,
    To(String),
}

impl TryFrom<String> for ChangeDirectory {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}

impl From<ChangeDirectory> for String {
    fn from(_: ChangeDirectory) -> Self {
        <ChangeDirectory as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Lib
////////////////////////////////////////////////////////////////////////////////////////////////////

fn mk_try_from<T: Method + Default>() -> impl Fn(String) -> Result<T, String> {
    move |s| {
        if s == T::method_name() {
            Ok(Default::default())
        } else {
            Err(s)
        }
    }
}
