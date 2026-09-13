// SPDX-License-Identifier: GPL-3.0-only

use chrono::Local;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const DEFAULT_EXCLUDED_DIRS: &[&str] = &[
    "$Recycle.Bin",
    ".VirtualBox",
    ".cache",
    ".cargo",
    ".copilot",
    ".git",
    ".github",
    ".gradle",
    ".idea",
    ".idlerc",
    ".lmstudio",
    ".local",
    ".platformio",
    ".rustup",
    ".service",
    ".venv",
    ".venv1",
    ".vscode",
    ".vscode-shared",
    "1Password",
    "AppData",
    "Arduino",
    "Backup",
    "EAgames",
    "EpicGames",
    "GOG Games",
    "GitHub",
    "Lib",
    "My Games",
    "OneDrive",
    "Paradox Interactive",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "Quantic Dream",
    "Rockstar Games",
    "Saved Games",
    "Steam",
    "SteamLibrary",
    "System32",
    "TECU3v3.6.0.6",
    "WebstormProjects",
    "Windows",
    "XboxGames",
    "bl-content",
    "bl-kernel",
    "bl-languages",
    "bl-plugins",
    "bl-themes",
    "cygwin",
    "debug",
    "game",
    "games",
    "goodbyedpi",
    "goodbyedpiV2",
    "id Software",
    "include",
    "llama.cpp",
    "mingw64",
    "plugin",
    "plugins",
    "python",
    "python3",
    "release",
    "settings",
    "sourcemod",
    "stable-diffusion",
    "steamapps",
    "wp-admin",
    "wp-content",
    "wp-includes",
    "x86_64",
    "zapret-discord-youtube-main",
    "zapret-main",
    "zapret2-main",
];

const DEFAULT_EXCLUDED_FILENAMES: &[&str] = &[
    ".gitconfig",
    ".gitignore",
    "AlbumArtSmall.jpg",
    "NTUSER.DAT",
    "README.md",
    "UnityCrashHandler64.exe",
    "cd.ico",
    "compile.exe",
    "cover.JPG",
    "desktop.ini",
    "favicon.ico",
    "favicon.png",
    "folder.jpg",
    "index.html",
    "install.exe",
    "main.rs",
    "readme.html",
    "spcomp.exe",
    "spcomp64.exe",
];

const DEFAULT_EXTENSIONS_WHITE_LIST: &[&str] = &[
    "7z", "avi", "backup", "chm", "csv", "djvu", "doc", "docx", "exe", "fb2", "gif", "htm", "html", "ico", "iso", "jpeg", "jpg", "log", "mov", "mp3", "mp4", "numbers", "odt", "pdf", "png", "pptx",
    "psd", "pxm", "rar", "sp", "txt", "xls", "zip",
];

const MIN_FILE_SIZE: u64 = 1024;
const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024;

struct Logger {
    writer: BufWriter<File>,
}

impl Logger {
    fn new(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { writer: BufWriter::new(file) })
    }

    fn log(&mut self, msg: &str) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let log_line = format!("[{}] {}\n", timestamp, msg);

        print!("{}", log_line);
        let _ = self.writer.write_all(log_line.as_bytes());
    }

    fn flush(&mut self) {
        let _ = self.writer.flush();
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}

fn is_valid_setting_line(line: &str) -> bool {
    if line.len() > 128 {
        return false;
    }

    let trimmed = line.trim_start_matches('\u{FEFF}').trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return false;
    }

    trimmed.chars().all(|c| {
        let code = c as u32;
        !(code <= 0x1F || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'))
    })
}

fn load_settings_set(file_path: &Path, default_list: &[&str], logger: &mut Logger) -> HashSet<String> {
    if let Some(parent) = file_path.parent()
        && !parent.exists()
        && let Err(e) = fs::create_dir_all(parent)
    {
        logger.log(&format!("Ошибка создания директории {}: {}", parent.display(), e));
        return default_list.iter().map(|s| s.to_lowercase()).collect();
    }

    if !file_path.exists() {
        logger.log(&format!("Файл {} не найден, создаём с настройками по умолчанию", file_path.display()));
        let content = default_list.join("\n");
        if let Err(e) = fs::write(file_path, content) {
            logger.log(&format!("Ошибка записи в файл {}: {}", file_path.display(), e));
        }
        return default_list.iter().map(|s| s.to_lowercase()).collect();
    }

    let file = match File::open(file_path) {
        Ok(f) => f,
        Err(e) => {
            logger.log(&format!("Ошибка открытия файла {}: {}", file_path.display(), e));
            return default_list.iter().map(|s| s.to_lowercase()).collect();
        }
    };

    let reader = BufReader::new(file);
    let mut set = HashSet::new();

    let mut valid_lines = Vec::new();
    let mut invalid_lines = false;

    for (idx, line_result) in reader.lines().enumerate() {
        if idx >= 1024 {
            logger.log(&format!("Достигнут лимит строк в файле {}", file_path.display()));
            invalid_lines = true;
            break;
        }

        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                logger.log(&format!("Ошибка чтения строки в {}: {}", file_path.display(), e));
                invalid_lines = true;
                continue;
            }
        };

        if !is_valid_setting_line(&line) {
            invalid_lines = true;
            continue;
        }

        set.insert(line.trim().to_lowercase());
        valid_lines.push(line);
    }

    if invalid_lines && !set.is_empty() {
        let content = valid_lines.join("\n");
        if let Err(e) = fs::write(file_path, content) {
            logger.log(&format!("Ошибка перезаписи файла {}: {}", file_path.display(), e));
        } else {
            logger.log(&format!("Файл {} содержал некорректные строки и был перезаписан", file_path.display()));
        }
    }

    if set.is_empty() {
        logger.log(&format!("Файл {} пуст или не содержит корректных строк. Используем умолчания", file_path.display()));
        default_list.iter().map(|s| s.to_lowercase()).collect()
    } else {
        set
    }
}

fn get_sha256(file_path: &Path) -> io::Result<String> {
    let mut file = File::open(file_path)?;
    let mut hasher = Sha256::new();

    let mut buffer = [0u8; 131072];

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    let bytes = hasher.finalize();
    let mut hex_str = String::with_capacity(bytes.len() * 2);
    for b in bytes.iter() {
        hex_str.push_str(&format!("{:02x}", b));
    }

    Ok(hex_str)
}

fn move_to_trash(path: &Path, logger: &mut Logger) -> bool {
    match trash::delete(path) {
        Ok(()) => {
            logger.log(&format!("Файл помещён в корзину: {:?}", path));
            true
        }
        Err(e) => {
            logger.log(&format!("Ошибка перемещения в корзину файла {:?}: {}", path, e));
            false
        }
    }
}

fn main() {
    let log_path = Path::new("process.log");
    let mut logger = match Logger::new(log_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Критическая ошибка: не удалось открыть лог-файл. {}", e);
            std::process::exit(1);
        }
    };

    let current_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            logger.log(&format!("Ошибка получения текущей директории: {}", e));
            return;
        }
    };

    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            logger.log(&format!("Критическая ошибка: не удалось получить путь к исполняемому файлу: {}", e));
            return;
        }
    };

    let settings_dir = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string()))
        .join("Documents")
        .join("hx_settings");

    let excluded_dirs = load_settings_set(&settings_dir.join("folders.txt"), DEFAULT_EXCLUDED_DIRS, &mut logger);
    let excluded_filenames = load_settings_set(&settings_dir.join("files.txt"), DEFAULT_EXCLUDED_FILENAMES, &mut logger);
    let included_extensions = load_settings_set(&settings_dir.join("extensions.txt"), DEFAULT_EXTENSIONS_WHITE_LIST, &mut logger);
    let delete_mode_set = load_settings_set(&settings_dir.join("delete.txt"), &["no"], &mut logger);

    let delete_mode = delete_mode_set.contains("yes");
    if delete_mode {
        logger.log("Режим: удаление");
    } else {
        logger.log("Режим: просмотр. Для включения режима удаления впишите 'yes' в файл delete.txt");
    }
    logger.log(&format!("Старт сканирования: {:?}", current_dir));
    logger.log(&format!(
        "Исключений загружено: папок: {}, файлов: {}. Разрешённых расширений файлов: {}",
        excluded_dirs.len(),
        excluded_filenames.len(),
        included_extensions.len()
    ));

    let mut files_by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();

    let walker = WalkDir::new(&current_dir).follow_links(false).into_iter();
    let filtered_entries = walker.filter_entry(|e| {
        if e.file_type().is_dir() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            return !excluded_dirs.contains(&name);
        }
        true
    });

    for entry in filtered_entries.filter_map(|e| e.ok()) {
        let path = entry.path();

        if path.is_symlink() {
            continue;
        }

        if !path.is_file() {
            continue;
        }

        if path == exe_path || path.ends_with(log_path) {
            continue;
        }

        let file_name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();

        if excluded_filenames.contains(&file_name) {
            continue;
        }

        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            if !included_extensions.contains(&ext_str) {
                continue;
            }
        } else {
            continue;
        }

        if let Ok(meta) = entry.metadata() {
            let size = meta.len();
            if size < MIN_FILE_SIZE {
                continue;
            }

            if size > MAX_FILE_SIZE {
                continue;
            }

            files_by_size.entry(size).or_default().push(path.to_path_buf());
        }
    }

    logger.log(&format!("Сканирование завершено. Найдено групп файлов по размеру: {}", files_by_size.len()));

    let mut duplicates_found = 0;
    let mut hashes: HashMap<String, PathBuf> = HashMap::new();

    for (size, files) in files_by_size {
        if files.len() == 1 {
            continue;
        }

        for path in files {
            match get_sha256(&path) {
                Ok(hash) => {
                    if let Some(first_seen) = hashes.get(&hash) {
                        let name_len_1 = path.to_string_lossy().len();
                        let name_len_2 = first_seen.to_string_lossy().len();

                        if name_len_2 > name_len_1 {
                            if delete_mode {
                                if move_to_trash(first_seen, &mut logger) {
                                    hashes.insert(hash, path.clone());
                                }
                            } else {
                                logger.log(&format!("Оригинал: {:?} ({} байт) -> Дубликат: {:?}", path, size, first_seen));
                                hashes.insert(hash, path.clone());
                            }
                        } else {
                            if delete_mode {
                                let _ = move_to_trash(&path, &mut logger);
                            } else {
                                logger.log(&format!("Оригинал: {:?} ({} байт) -> Дубликат: {:?}", first_seen, size, path));
                            }
                        }

                        duplicates_found += 1;
                    } else {
                        hashes.insert(hash, path);
                    }
                }
                Err(e) => {
                    logger.log(&format!("Ошибка хэширования {:?}: {}", path, e));
                }
            }
        }
    }

    logger.log(&format!("Работа завершена. Обработано дубликатов: {}", duplicates_found));
    logger.flush();
}
