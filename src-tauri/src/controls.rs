use crate::{
    lighting,
    linux::{drop_pkexec_privileges, emit, request_line},
    turbo, Service, WorkerEvent,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{io, sync::mpsc, thread};

#[derive(Deserialize)]
struct Command {
    service: Service,
    request: Option<Value>,
}

fn reply(service: Service, kind: &'static str, data: Value) -> io::Result<()> {
    emit(&WorkerEvent {
        service,
        kind,
        data,
    })
}

pub fn run() -> io::Result<()> {
    // 一次授权只打开已经校验的灯控设备和固定的睿频入口。处理请求前恢复普通用户身份。
    let mut lighting = lighting::Session::open();
    let turbo = turbo::Session::open();
    drop_pkexec_privileges()?;
    emit(&json!({"ready": true}))?;
    let (send, requests) = mpsc::channel();
    let (monitor, turbo_error) = match turbo {
        Ok(session) => (
            Some(thread::spawn(move || {
                if let Err(error) = session.run(requests, |data| {
                    reply(Service::Turbo, "message", data.clone())
                }) {
                    let _ = reply(Service::Turbo, "exit", json!({"detail": error.to_string()}));
                }
            })),
            None,
        ),
        Err(error) => (None, Some(error.to_string())),
    };

    let result = (|| {
        let mut input = io::stdin().lock();
        while let Some(line) = request_line(&mut input)? {
            let command: Command = serde_json::from_slice(&line)?;
            match command.service {
                Service::Lighting => {
                    let starting = command.request.is_none();
                    let result = lighting
                        .as_mut()
                        .map_err(|error| error.to_string())
                        .and_then(|session| {
                            match command.request {
                                Some(request) => session.request(request),
                                None => session.status(),
                            }
                            .map_err(|error| error.to_string())
                        });
                    match result {
                        Ok(data) => reply(Service::Lighting, "message", data)?,
                        Err(detail) if starting => {
                            reply(Service::Lighting, "exit", json!({"detail": detail}))?
                        }
                        Err(error) => reply(Service::Lighting, "message", json!({"error": error}))?,
                    }
                }
                Service::Turbo => {
                    if let Some(detail) = turbo_error.as_ref() {
                        reply(Service::Turbo, "exit", json!({"detail": detail}))?;
                    } else {
                        let turbo::Request::TurboPolicy { enabled } =
                            serde_json::from_value(command.request.unwrap_or_default())?;
                        if send.send(enabled).is_err() {
                            reply(
                                Service::Turbo,
                                "exit",
                                json!({"detail": "睿频控制会话已结束"}),
                            )?;
                        }
                    }
                }
                Service::Cooler => return Err(io::Error::other("未知的硬件操作")),
            }
        }
        Ok(())
    })();
    // EOF 让睿频线程先恢复此前的状态，再关闭共用会话。
    drop(send);
    if let Some(monitor) = monitor {
        let _ = monitor.join();
    }
    result
}
