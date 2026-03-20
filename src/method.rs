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
    Dispatch {
        method: Dispatch,
        params: <Dispatch as Method>::Param,
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
// Dispatch method
// transform から呼ばれ、現在のモードに応じた fzf アクション文字列を返す
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Dispatch;

impl Method for Dispatch {
    type Param = DispatchParam;
    type Response = DispatchResp;
    fn method_name() -> &'static str {
        "dispatch"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Dispatch {
            method: Dispatch,
            params,
        }
    }
}

#[derive(Serialize, Deserialize, clap::Parser, Clone, Debug)]
pub struct DispatchParam {
    /// キー名 (例: "change-mode:buffer", "enter", "ctrl-y")
    pub key: String,
    pub query: String,
    pub item: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct DispatchResp {
    /// fzf が実行するアクション文字列 (例: "reload(...)+change-prompt(...)")
    pub action: String,
}

impl TryFrom<String> for Dispatch {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}

impl From<Dispatch> for String {
    fn from(_: Dispatch) -> Self {
        <Dispatch as Method>::method_name().to_string()
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

#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug)]
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
    pub header: Option<String>,
    pub items: Vec<String>,
    pub is_last: bool,
}

impl LoadResp {
    pub fn new_with_default_header(items: Vec<String>) -> Self {
        let pwd = std::env::current_dir().unwrap().into_os_string();
        Self {
            header: Some(format!("[{}]", pwd.to_string_lossy())),
            items,
            is_last: true,
        }
    }
    pub fn error(err: impl ToString) -> Self {
        Self {
            header: Some("[error]".to_string()),
            items: vec![err.to_string()],
            is_last: true,
        }
    }
    pub fn wip_with_default_header(items: Vec<String>) -> Self {
        let pwd = std::env::current_dir().unwrap().into_os_string();
        Self {
            header: Some(format!("[{}]", pwd.to_string_lossy())),
            items,
            is_last: false,
        }
    }
    pub fn last() -> Self {
        Self {
            header: None,
            items: vec![],
            is_last: true,
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
