use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use unicode_width::UnicodeWidthStr;

use crate::bindings;
use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::state::State;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;

#[derive(Clone)]
pub struct ProcessCompose;

impl ProcessCompose {
    pub fn new() -> Self {
        Self
    }
}

struct Item {
    process: String,
}

impl Item {
    fn parse(item: String) -> Self {
        Self { process: item }
    }
}

mod dto {
    #[derive(serde::Deserialize, Debug)]
    pub(crate) struct Processes {
        pub(crate) data: Vec<Process>,
    }
    #[derive(serde::Deserialize, Debug)]
    pub(crate) struct Process {
        pub(crate) name: String,
    }
    #[derive(serde::Deserialize, Debug)]
    pub struct Logs {
        pub(crate) logs: Vec<String>,
    }
}

impl ModeDef for ProcessCompose {
    fn name(&self) -> &'static str {
        "process-compose"
    }
    fn load<'a>(
        &'a self,
        _config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        Box::pin(async_stream::stream! {
            let host = get_host()?;
            let processes = reqwest::get(format!("{host}/processes"))
                .await?
                .json::<dto::Processes>()
                .await?;
            let mut items = processes
                .data
                .into_iter()
                .map(|p| p.name)
                .collect::<Vec<_>>();
            items.sort();
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview<'a>(
        &self,
        _config: &Config,
        win: &'a PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>> {
        async move {
            let Item { process } = Item::parse(item);

            // 最後の高々lines行だけログを取得する
            let host = get_host()?;
            let lines = win.lines;
            let limit = 0; // 0 will get all the lines till the end
            let logs = reqwest::get(format!("{host}/process/logs/{process}/{lines}/{limit}"))
                .await?
                .json::<dto::Logs>()
                .await?
                .logs;

            // 折返しを考慮した上で再度高々lines行だけ残す
            let mut logs = logs
                .iter()
                .flat_map(|s| wrap(s, win.columns))
                .collect::<Vec<_>>();
            let offset = if logs.len() > lines {
                logs.len() - lines
            } else {
                0
            };
            let logs = logs.split_off(offset);

            let message = logs.join("\n");
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                select_and_execute!{b, |_mode,_config,_state,_query,item|
                    "restart" => {
                        restart(Item::parse(item)).await?;
                        Ok(())
                    },
                    "start" => {
                        start(Item::parse(item)).await?;
                        Ok(())
                    },
                    "stop" => {
                        stop(Item::parse(item)).await?;
                        Ok(())
                    },
                },
                b.reload(),
            ],
            "ctrl-e" => [
                execute_silent!(b, |_mode,_config,_state,_query,item| {
                    restart(Item::parse(item)).await?;
                    Ok(())
                }),
                b.reload()
            ],
            "right" => [
                b.reload()
            ],
        }
    }
}

fn get_host() -> Result<String> {
    std::env::var("FZFW_PROCESS_COMPOSE_HOST").map_err(|_| anyhow!("No host"))
}

async fn restart(item: Item) -> Result<()> {
    let Item { process } = item;
    let host = get_host()?;
    let client = reqwest::Client::new();
    let _processes = client
        .post(format!("{host}/process/restart/{process}"))
        .send()
        .await?;
    Ok(())
}

async fn start(item: Item) -> Result<()> {
    let Item { process } = item;
    let host = get_host()?;
    let client = reqwest::Client::new();
    let _processes = client
        .post(format!("{host}/process/start/{process}"))
        .send()
        .await?;
    Ok(())
}

async fn stop(item: Item) -> Result<()> {
    let Item { process } = item;
    let host = get_host()?;
    let client = reqwest::Client::new();
    let _processes = client
        .post(format!("{host}/process/stop/{process}"))
        .send()
        .await?;
    Ok(())
}

// wrap("foobar", 3) => ["foo", "bar"]
// wrap("犬猫", 3) => ["犬", "猫"]
fn wrap(s: &str, columns: usize) -> Vec<String> {
    let mut result = Vec::new();
    let mut chunk = String::new();
    let mut width = 0;
    for c in s.chars() {
        let c_width = UnicodeWidthStr::width(c.to_string().as_str());
        if width + c_width > columns {
            result.push(chunk);
            chunk = String::new();
            width = 0;
        }
        chunk.push(c);
        width += c_width;
    }
    if !chunk.is_empty() {
        result.push(chunk);
    }
    result
}
