use serde::Serialize;
use std::{
    env, fs,
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    ptr,
};

pub fn drop_pkexec_privileges() -> io::Result<()> {
    let Some(uid) = env::var_os("PKEXEC_UID").filter(|_| unsafe { libc::geteuid() } == 0) else {
        return Ok(());
    };
    let uid: libc::uid_t = uid
        .to_str()
        .and_then(|value| value.parse().ok())
        .filter(|uid| *uid != 0)
        .ok_or_else(|| io::Error::other("无法识别发起认证的普通用户"))?;
    // 在单线程助手中完成身份切换，之后只使用已经打开的设备句柄。
    let entry = unsafe { libc::getpwuid(uid) };
    if entry.is_null() {
        return Err(io::Error::other("无法读取认证用户的组信息"));
    }
    let gid = unsafe { (*entry).pw_gid };
    if unsafe { libc::setgroups(0, ptr::null()) } < 0
        || unsafe { libc::setresgid(gid, gid, gid) } < 0
        || unsafe { libc::setresuid(uid, uid, uid) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn text(path: impl AsRef<Path>) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let value = String::from_utf8_lossy(&bytes);
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub fn entries(path: impl AsRef<Path>) -> Vec<PathBuf> {
    let Ok(iter) = fs::read_dir(path) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = iter
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort_unstable();
    paths
}

pub fn name(path: &Path) -> &str {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
}

pub fn emit(value: &impl Serialize) -> io::Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, value)?;
    out.write_all(b"\n")?;
    out.flush()
}

pub fn request_line(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    const MAX_LINE: u64 = 1024;
    let mut line = Vec::with_capacity(256);
    if input.take(MAX_LINE + 1).read_until(b'\n', &mut line)? == 0 {
        return Ok(None);
    }
    // 输入是本应用的专属逐行管道。拒绝截断的或超长的请求。
    if line.len() as u64 > MAX_LINE || line.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "硬件请求过长或缺少换行",
        ));
    }
    Ok(Some(line))
}

pub fn rgb(value: &str) -> Result<[u8; 3], &'static str> {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        return Err("颜色应为 #RRGGBB");
    }
    let byte = |offset| u8::from_str_radix(&value[offset..offset + 2], 16).unwrap_or_default();
    Ok([byte(1), byte(3), byte(5)])
}
