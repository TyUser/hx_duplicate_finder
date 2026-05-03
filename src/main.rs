use chrono::Local;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use trash;
use walkdir::WalkDir;

const DEFAULT_EXCLUDED_DIRS: &[&str] = &[
    "$Recycle.Bin",
    ".VirtualBox",
    ".cache",
    ".cargo",
    ".git",
    ".github",
    ".gradle",
    ".idea",
    ".idlerc",
    ".lmstudio",
    ".platformio",
    ".rustup",
    ".service",
    ".venv",
    ".venv1",
    ".vscode",
    "1Password",
    "AppData",
    "Arduino",
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
];

const DEFAULT_EXCLUDED_FILENAMES: &[&str] = &[
    ".gitconfig",
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
    "readme.html",
    "spcomp.exe",
    "spcomp64.exe",
];

const DEFAULT_EXCLUDED_EXTENSIONS_WHITE_LIST: &[&str] = &[
    "7z", "avi", "backup", "chm", "djvu", "doc", "docx", "exe", "fb2", "gif", "htm", "html", "ico", "jpeg", "jpg", "log", "mov", "mp3", "mp4", "numbers", "odt", "pdf", "png", "pptx", "psd", "pxm",
    "rar", "sp", "txt", "xls", "zip",
];

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

fn is_safe_exclusion_line(line: &str) -> bool {
    let trimmed = line.trim_start_matches('\u{FEFF}').trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return false;
    }

    trimmed.chars().all(|c| {
        let code = c as u32;
        !(code <= 0x1F || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'))
    })
}

fn load_exclusions(file_path: &Path, default_list: &[&str], logger: &mut Logger) -> HashSet<String> {
    if let Some(parent) = file_path.parent() {
        if !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                logger.log(&format!("Ошибка создания директории {}: {}", parent.display(), e));
                return default_list.iter().map(|s| s.to_lowercase()).collect();
            }
        }
    }

    if !file_path.exists() {
        logger.log(&format!("Файл {} не найден, создаём с настройками по умолчанию.", file_path.display()));
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

    for (idx, line_result) in reader.lines().enumerate() {
        if idx >= 1024 {
            logger.log(&format!("Достигнут лимит строк в файле {}", file_path.display()));
            break;
        }

        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                logger.log(&format!("Ошибка чтения строки в {}: {}", file_path.display(), e));
                continue;
            }
        };

        if line.len() > 128 || !is_safe_exclusion_line(&line) {
            continue;
        }

        set.insert(line.trim().to_lowercase());
    }

    if set.is_empty() {
        logger.log(&format!("Файл {} пуст или не содержит валидных строк. Используем умолчания.", file_path.display()));
        default_list.iter().map(|s| s.to_lowercase()).collect()
    } else {
        set
    }
}

fn read_delete_config(config_path: &Path, logger: &mut Logger) -> String {
    if let Some(parent) = config_path.parent() {
        if !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                logger.log(&format!("Ошибка создания директории {}: {}", parent.display(), e));
                return "null".to_string();
            }
        }
    }

    if !config_path.exists() {
        logger.log(&format!(
            "Файл {} не найден, создаём с настройкой по умолчанию 'null'. Для автоматического удаления файлов измени на 'yes'",
            config_path.display()
        ));
        let content = "null";
        if let Err(e) = fs::write(config_path, content) {
            logger.log(&format!("Ошибка записи в файл {}: {}", config_path.display(), e));
        }
        return "null".to_string();
    }

    let file = match File::open(config_path) {
        Ok(f) => f,
        Err(e) => {
            logger.log(&format!("Ошибка открытия файла {}: {}", config_path.display(), e));
            return "null".to_string();
        }
    };

    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    if let Some(Ok(line)) = lines.next() {
        let trimmed = line.trim().to_lowercase();
        if trimmed == "yes" { "yes".to_string() } else { "null".to_string() }
    } else {
        logger.log(&format!("Файл {} пуст, используем 'null'.", config_path.display()));
        "null".to_string()
    }
}

fn get_sha256(file_path: &Path) -> io::Result<String> {
    let mut file = File::open(file_path)?;
    let mut hasher = Sha256::new();

    let mut buffer = [0u8; 65536];

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

fn move_to_trash_if_exists(path: &Path, logger: &mut Logger) -> bool {
    if !path.exists() {
        logger.log(&format!("Пропуск удаления: файл не существует: {:?}", path));
        return false;
    }

    if !path.is_file() {
        logger.log(&format!("Пропуск удаления: путь не является файлом: {:?}", path));
        return false;
    }

    match trash::delete(path) {
        Ok(()) => {
            logger.log(&format!("Файл помещен в корзину: {:?}", path));
            true
        }
        Err(e) => {
            logger.log(&format!("Ошибка перемещения в корзину {:?}: {}", path, e));
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
            return;
        }
    };

    let current_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            logger.log(&format!("Ошибка получения текущей директории: {}", e));
            return;
        }
    };

    let exe_path = std::env::current_exe().unwrap_or_default();

    let exclusions_dir = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string()))
        .join("Documents")
        .join("hx_settings");

    let excluded_dirs = load_exclusions(&exclusions_dir.join("folders.txt"), DEFAULT_EXCLUDED_DIRS, &mut logger);
    let excluded_filenames = load_exclusions(&exclusions_dir.join("files.txt"), DEFAULT_EXCLUDED_FILENAMES, &mut logger);
    let included_extensions = load_exclusions(&exclusions_dir.join("extensions.txt"), DEFAULT_EXCLUDED_EXTENSIONS_WHITE_LIST, &mut logger);

    let delete_config_path = exclusions_dir.join("delete.txt");
    let delete_mode = read_delete_config(&delete_config_path, &mut logger);
    if delete_mode == "yes" {
        logger.log(&format!("Режим: удаление активно"));
    } else {
        logger.log(&format!("Режим: только просмотр"));
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
            continue; // файлы без расширения пропускаем
        }

        if let Ok(meta) = entry.metadata() {
            let size = meta.len();
            if size < 1024 {
                continue;
            }

            if size > 20 * 1024 * 1024 * 1024 {
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
                    if let Some(original) = hashes.get(&hash) {
                        let path_len = path.to_string_lossy().len();
                        let orig_len = original.to_string_lossy().len();

                        if orig_len > path_len {
                            if delete_mode == "yes" {
                                if move_to_trash_if_exists(original, &mut logger) {
                                    hashes.insert(hash, path.clone());
                                }
                            } else {
                                logger.log(&format!("Оригинал: {:?} ({} байт) -> Дубликат: {:?}", path, size, original));
                            }
                        } else {
                            if delete_mode == "yes" {
                                let _ = move_to_trash_if_exists(&path, &mut logger);
                            } else {
                                logger.log(&format!("Оригинал: {:?} ({} байт) -> Дубликат: {:?}", original, size, path));
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

    logger.log(&format!("Работа завершена. Найдено дубликатов: {}", duplicates_found));
    logger.flush();
}
