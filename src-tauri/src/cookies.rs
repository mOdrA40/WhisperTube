use std::{fs, path::PathBuf};

pub fn args(cookies_path: Option<&str>) -> Result<Vec<String>, String> {
    let Some(path) = cookies_path.filter(|path| !path.is_empty()) else {
        return Ok(Vec::new());
    };

    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err("File cookies.txt tidak ditemukan atau tidak bisa dibaca.".into());
    }
    let size = fs::metadata(&path)
        .map_err(|_| "File cookies.txt tidak ditemukan atau tidak bisa dibaca.".to_string())?
        .len();
    if size > 256 * 1024 * 1024 {
        return Err("File cookies.txt terlalu besar untuk diproses dengan aman.".into());
    }

    Ok(vec![
        "--cookies".into(),
        path.to_string_lossy().into_owned(),
    ])
}
