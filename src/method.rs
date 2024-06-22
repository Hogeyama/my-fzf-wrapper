use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;

use crate::utils::fzf::PreviewWindow;

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
    Preview {
        method: Preview,
        params: <Preview as Method>::Param,
        preview_window: PreviewWindow,
    },
    Execute {
        method: Execute,
        params: <Execute as Method>::Param,
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
// Preview method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Preview;

#[derive(Serialize, Deserialize, clap::Parser, Default, Clone, Debug)]
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
        let preview_window = PreviewWindow::from_env().unwrap();
        Request::Preview {
            method: Preview,
            params,
            preview_window,
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

#[derive(Serialize, Deserialize, clap::Parser, Default, Clone, Debug)]
pub struct ChangeModeParam {
    pub mode: String,
    pub query: Option<String>,
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

// clap impl
////////////

impl clap::Args for ChangeDirectoryParam {
    fn augment_args(cmd: clap::Command<'_>) -> clap::Command<'_> {
        ChangeDirectoryCommandParam::augment_args(cmd)
    }
    fn augment_args_for_update(cmd: clap::Command<'_>) -> clap::Command<'_> {
        ChangeDirectoryCommandParam::augment_args(cmd)
    }
}

impl clap::FromArgMatches for ChangeDirectoryParam {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        ChangeDirectoryCommandParam::from_arg_matches(matches).map(|param| param.into())
    }
    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        let mut self_ = Into::<ChangeDirectoryCommandParam>::into(self.clone());
        self_.update_from_arg_matches(matches)?;
        *self = self_.into();
        Ok(())
    }
}

#[derive(Serialize, Deserialize, clap::Parser, Clone, Debug)]
struct ChangeDirectoryCommandParam {
    #[clap(long, group = "input")]
    to_parent: bool,
    #[clap(long, group = "input")]
    to_last_file_dir: bool,
    #[clap(long, group = "input")]
    dir: Option<String>,
}

impl From<ChangeDirectoryCommandParam> for ChangeDirectoryParam {
    fn from(param: ChangeDirectoryCommandParam) -> Self {
        if param.to_parent {
            Self::ToParent
        } else if param.to_last_file_dir {
            Self::ToLastFileDir
        } else if let Some(dir) = param.dir {
            Self::To(dir)
        } else {
            unreachable!()
        }
    }
}

impl From<ChangeDirectoryParam> for ChangeDirectoryCommandParam {
    fn from(param: ChangeDirectoryParam) -> Self {
        match param {
            ChangeDirectoryParam::ToParent => Self {
                to_parent: true,
                to_last_file_dir: false,
                dir: None,
            },
            ChangeDirectoryParam::ToLastFileDir => Self {
                to_parent: false,
                to_last_file_dir: true,
                dir: None,
            },
            ChangeDirectoryParam::To(dir) => Self {
                to_parent: false,
                to_last_file_dir: false,
                dir: Some(dir),
            },
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Execute method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Execute;

impl Method for Execute {
    type Param = ExecuteParam;
    type Response = ();
    fn method_name() -> &'static str {
        "execute_cb"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Execute {
            method: Execute,
            params,
        }
    }
}

#[derive(Serialize, Deserialize, clap::Parser, Clone, Debug)]
pub struct ExecuteParam {
    pub registered_name: String,
    pub query: String,
    pub item: String,
}

impl TryFrom<String> for Execute {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}

impl From<Execute> for String {
    fn from(_: Execute) -> Self {
        <Execute as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Load method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Load;

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

#[derive(Serialize, Deserialize, clap::Parser, Clone, Debug)]
pub struct LoadParam {
    pub registered_name: String,
    pub query: String,
    pub item: Option<String>,
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
