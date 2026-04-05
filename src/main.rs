// ============================================
// 程序: gen-changelog
// 描述: 根据 Git 提交记录生成 CHANGELOG.md
// 用法: cargo run -- [起始版本] [目标版本]
// ============================================

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::path::Path;

// ============================================
// 颜色输出
// ============================================

mod colors {
    pub fn red(s: &str) -> String { format!("\x1b[31m{}\x1b[0m", s) }
    pub fn green(s: &str) -> String { format!("\x1b[32m{}\x1b[0m", s) }
    pub fn yellow(s: &str) -> String { format!("\x1b[33m{}\x1b[0m", s) }
}

// ============================================
// 配置常量
// ============================================

const CHANGELOG_FILE: &str = "CHANGELOG.md";

// 需要从提交描述中移除的 CI 标记
const SKIP_PATTERNS: &[&str] = &[
    r"\[skip ci\]",
    r"\[ci skip\]",
    r"\[skip actions\]",
    r"\[actions skip\]",
    r"\[skip travis\]",
    r"\[skip circleci\]",
    r"\[skip azure\]",
    r"\[skip gitlab\]",
    r"\[skip cd\]",
    r"ci skip",
    r"skip ci",
];

// 分类优先级顺序
const CATEGORY_ORDER: &[&str] = &[
    "### BREAKING CHANGES",
    "### Added",
    "### Changed",
    "### Deprecated",
    "### Removed",
    "### Fixed",
    "### Security",
    "### Performance",
    "### Refactored",
    "### Docs",
    "### Style",
    "### Test",
    "### Build",
    "### CI",
    "### Chore",
    "### I18n",
    "### Config",
    "### Migration",
    "### Release",
    "### Reverted",
    "### Other",
];

// ============================================
// 数据结构
// ============================================

#[derive(Debug, Clone)]
struct CommitInfo {
    hash: String,
    author: String,
    full_msg: String,
}

#[derive(Debug, Clone)]
struct ParsedCommit {
    type_: String,
    scope: String,
    description: String,
    is_breaking: bool,
}

// ============================================
// Git 操作
// ============================================

/// 执行 Git 命令并返回输出
fn git_command(args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("执行 git 命令失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 获取远程仓库 URL 并转换为 HTTPS 格式
fn get_repo_url() -> String {
    let remote_url = match git_command(&["remote", "get-url", "origin"]) {
        Ok(url) => url,
        Err(_) => return String::new(),
    };

    // SSH 格式 (git@)
    let re = Regex::new(r"git@([^:]+):(.+)\.git$").unwrap();
    if let Some(caps) = re.captures(&remote_url) {
        return format!("https://{}/{}", &caps[1], &caps[2]);
    }

    // HTTPS 格式
    let re = Regex::new(r"https://([^/]+)/(.+)\.git$").unwrap();
    if let Some(caps) = re.captures(&remote_url) {
        return format!("https://{}/{}", &caps[1], &caps[2]);
    }

    // 其他格式，去除 .git 后缀
    remote_url.trim_end_matches(".git").to_string()
}

/// 获取仓库 URL（全局缓存）
fn get_repo_url_cached() -> &'static str {
    use std::sync::OnceLock;
    static REPO_URL: OnceLock<String> = OnceLock::new();
    REPO_URL.get_or_init(|| get_repo_url())
}

/// 生成提交链接
fn get_commit_link(hash: &str) -> String {
    let repo_url = get_repo_url_cached();
    if repo_url.is_empty() || hash.is_empty() {
        return hash.to_string();
    }
    format!("[{}]({}/commit/{})", hash, repo_url, hash)
}

/// 获取当前日期
fn get_current_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 移除提交描述中的 CI 跳过标记
fn clean_description(desc: &str) -> String {
    let mut cleaned = desc.to_string();
    for pattern in SKIP_PATTERNS {
        let re = Regex::new(&format!("(?i){}", pattern)).unwrap();
        cleaned = re.replace_all(&cleaned, "").to_string();
    }
    cleaned.trim().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 解析提交信息（支持 scope 可选）
fn parse_commit(msg: &str) -> Option<ParsedCommit> {
    // 匹配格式: type(scope)!: description 或 type!: description 或 type: description
    // 支持的类型: feat, fix, docs, style, refactor, perf, test, chore, ci, build,
    //            revert, security, hotfix, i18n, typo, config, migration, release
    let re = Regex::new(r"^(feat|fix|docs|style|refactor|perf|test|chore|ci|build|revert|security|hotfix|i18n|typo|config|migration|release)(?:\(([a-zA-Z0-9/_-]+)\))?(!)?:\s+(.*)$").unwrap();

    let caps = re.captures(msg)?;

    let type_ = caps[1].to_string();
    let scope = caps.get(2).map_or("", |m| m.as_str()).to_string();
    let is_breaking = caps.get(3).is_some() || msg.contains("BREAKING CHANGE");
    let description = clean_description(&caps[4]);

    Some(ParsedCommit {
        type_,
        scope,
        description,
        is_breaking,
    })
}

/// 获取类型对应的分类
fn get_category_for_type(type_: &str, is_breaking: bool) -> &'static str {
    if is_breaking {
        return "### BREAKING CHANGES";
    }

    match type_ {
        "feat" => "### Added",
        "fix" => "### Fixed",
        "docs" => "### Docs",
        "style" => "### Style",
        "refactor" => "### Refactored",
        "perf" => "### Performance",
        "test" => "### Test",
        "chore" => "### Chore",
        "ci" => "### CI",
        "build" => "### Build",
        "revert" => "### Reverted",
        "security" => "### Security",
        "hotfix" => "### Fixed",
        "i18n" => "### I18n",
        "typo" => "### Other",
        "config" => "### Config",
        "migration" => "### Migration",
        "release" => "### Release",
        _ => "### Other",
    }
}

/// 检查是否应该忽略的提交（wip, draft）
fn should_ignore(msg: &str) -> bool {
    let lower_msg = msg.to_lowercase();
    let re = Regex::new(r"^(wip|draft)[:\s\(]|\[(wip|draft)\]").unwrap();
    re.is_match(&lower_msg)
}

/// 获取提交信息
fn get_commit_info(msg: &str, range: &str) -> Option<CommitInfo> {
    let escaped_msg = msg.replace('"', "\\\"");
    let output = git_command(&[
        "log",
        &format!("--pretty=format:%h|%an|%s"),
        range,
        "--fixed-strings",
        "-F",
        &format!("--grep={}", escaped_msg),
        "-1",
    ]).ok()?;

    if output.is_empty() {
        return None;
    }

    let parts: Vec<&str> = output.splitn(3, '|').collect();
    if parts.len() < 3 {
        return None;
    }

    Some(CommitInfo {
        hash: parts[0].to_string(),
        author: parts[1].to_string(),
        full_msg: parts[2].to_string(),
    })
}

// ============================================
// CHANGELOG 生成
// ============================================

/// 收集提交并分类
fn collect_commits(range: &str) -> HashMap<String, Vec<String>> {
    let mut categories: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen = HashSet::new();

    // 获取所有提交（按时间正序）
    let commits_output = match git_command(&["log", "--pretty=format:%s", "--reverse", range]) {
        Ok(output) => output,
        Err(e) => {
            eprintln!("{}", colors::red(&format!("获取提交失败: {}", e)));
            return categories;
        }
    };

    for msg in commits_output.lines() {
        if msg.is_empty() {
            continue;
        }

        if should_ignore(msg) {
            println!("{}", colors::yellow(&format!("忽略提交: {}", msg)));
            continue;
        }

        let commit_info = match get_commit_info(msg, range) {
            Some(info) => info,
            None => continue,
        };

        let original_msg = msg.to_string();

        // 去重
        let clean_key = clean_description(&commit_info.full_msg);
        if seen.contains(&clean_key) {
            continue;
        }
        seen.insert(clean_key);

        let (category, mut entry) = if let Some(parsed) = parse_commit(&commit_info.full_msg) {
            let category = get_category_for_type(&parsed.type_, parsed.is_breaking);
            let mut entry = format!("- {}", parsed.description);
            if !parsed.scope.is_empty() {
                entry.push_str(&format!(" `{}`", parsed.scope));
            }
            (category.to_string(), entry)
        } else {
            // 非标准格式：直接使用原始提交信息
            ("### Other".to_string(), format!("- {}", original_msg))
        };

        // 添加链接和作者
        let link = get_commit_link(&commit_info.hash);
        entry.push_str(&format!(" ({})", link));
        if !commit_info.author.is_empty() {
            entry.push_str(&format!(" @{}", commit_info.author));
        }

        categories.entry(category).or_insert_with(Vec::new).push(entry);
    }

    categories
}

/// 生成版本内容
fn generate_version_content(version: &str, range: &str) -> String {
    let date = get_current_date();
    let mut content = format!("## [{}] - {}\n\n", version, date);

    let categories = collect_commits(range);

    let mut has_content = false;

    for cat in CATEGORY_ORDER {
        if let Some(entries) = categories.get(*cat) {
            if !entries.is_empty() {
                content.push_str(&format!("{}\n\n", cat));
                content.push_str(&entries.join("\n"));
                content.push_str("\n\n");
                has_content = true;
            }
        }
    }

    if !has_content {
        content.push_str("### Other\n\n- 无显著变更\n\n");
    }

    content
}

// ============================================
// 文件操作
// ============================================

/// 获取现有版本列表
fn get_existing_versions() -> HashSet<String> {
    let content = match fs::read_to_string(CHANGELOG_FILE) {
        Ok(c) => c,
        Err(_) => return HashSet::new(),
    };

    let re = Regex::new(r"^## \[([0-9]+\.[0-9]+\.[0-9]+)\]").unwrap();
    content.lines()
        .filter_map(|line| re.captures(line))
        .map(|caps| caps[1].to_string())
        .collect()
}

/// 备份现有 CHANGELOG
fn backup_changelog() -> io::Result<()> {
    if Path::new(CHANGELOG_FILE).exists() {
        let backup_file = format!("{}.bak", CHANGELOG_FILE);
        fs::copy(CHANGELOG_FILE, &backup_file)?;
        println!("{}", colors::green(&format!("✓ 已备份 {} 到 {}", CHANGELOG_FILE, backup_file)));
    }
    Ok(())
}

/// 更新 CHANGELOG 文件
fn update_changelog(new_content: &str) -> io::Result<()> {
    backup_changelog()?;

    if !Path::new(CHANGELOG_FILE).exists() {
        let header = "# Changelog

所有值得注意的变更都将记录在此文件。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，
版本遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

";
        fs::write(CHANGELOG_FILE, format!("{}{}", header, new_content))?;
        println!("{}", colors::green(&format!("✓ 已创建 {}", CHANGELOG_FILE)));
        return Ok(());
    }

    let content = fs::read_to_string(CHANGELOG_FILE)?;

    // 确保有头部
    let final_content = if !content.contains("# Changelog") {
        let header = "# Changelog

所有值得注意的变更都将记录在此文件。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，
版本遵循 [语义化版本](https://semver.org/lang/zh-CN/).

";
        format!("{}{}", header, content)
    } else {
        content
    };

    // 找到第一个版本块的位置
    let re = Regex::new(r"^## \[[0-9]").unwrap();
    let insert_line = final_content.lines()
        .position(|line| re.is_match(line));

    let lines: Vec<&str> = final_content.lines().collect();

    let new_lines = if let Some(line_num) = insert_line {
        let before = &lines[0..line_num];
        let after = &lines[line_num..];
        let mut result: Vec<String> = before.iter().map(|s| s.to_string()).collect();
        result.push(new_content.to_string());
        result.push(String::new());
        result.extend(after.iter().map(|s| s.to_string()));
        result
    } else {
        let mut result: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        result.push(String::new());
        result.push(new_content.to_string());
        result
    };

    fs::write(CHANGELOG_FILE, new_lines.join("\n"))?;
    println!("{}", colors::green(&format!("✓ 已更新 {}", CHANGELOG_FILE)));

    Ok(())
}

// ============================================
// 主函数
// ============================================

#[derive(Default)]
struct Options {
    preview: bool,
    force: bool,
    start_version: String,
    end_version: String,
}

fn parse_args() -> Options {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = Options::default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "-p" | "--preview" => {
                options.preview = true;
                i += 1;
            }
            "-f" | "--force" => {
                options.force = true;
                i += 1;
            }
            arg => {
                if options.start_version.is_empty() {
                    options.start_version = arg.to_string();
                } else if options.end_version.is_empty() {
                    options.end_version = arg.to_string();
                }
                i += 1;
            }
        }
    }

    options
}

fn print_help() {
    println!(r#"
用法: cargo run -- [选项] [起始版本] [目标版本]

根据 Git 提交记录生成或更新 CHANGELOG.md。

选项:
  -h, --help       显示帮助信息
  -p, --preview    预览模式，不写入文件
  -f, --force      强制覆盖已存在的版本

参数:
  起始版本         起始 Git tag 或 commit (默认: 最新的 tag)
  目标版本         目标版本号或 HEAD (默认: HEAD)

示例:
  cargo run --                                   # 使用最新 tag 到 HEAD
  cargo run -- v1.0.0                            # v1.0.0 到 HEAD
  cargo run -- v1.0.0 v1.1.0                     # v1.0.0 到 v1.1.0
  cargo run -- --preview                         # 预览将要生成的条目
  cargo run -- -f v1.0.0                         # 强制覆盖已存在的版本

支持的 PR 标题格式:
  feat: description              新功能（无 scope）
  feat(api): description         新功能（带 scope）
  fix!: description              破坏性修复
  ci: description                CI/CD 配置
  i18n: description              国际化
  typo: description              拼写错误（归入 Other）

忽略的提交类型:
  - wip: / WIP: / [wip] (开发中)
  - draft: / DRAFT: / [draft] (草稿)

自动移除的 CI 标记:
  - [skip ci], [ci skip], [skip actions], [actions skip]
"#);
}

fn check_git_repo() -> bool {
    Command::new("git")
        .args(&["rev-parse", "--git-dir"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn preview_changelog(start_version: &str, end_version: &str) {
    let (range, version) = if start_version.is_empty() {
        match git_command(&["describe", "--tags", "--abbrev=0"]) {
            Ok(latest_tag) => {
                let range = format!("{}..{}", latest_tag, if end_version.is_empty() { "HEAD" } else { end_version });
                (range, latest_tag)
            }
            Err(_) => {
                let range = if end_version.is_empty() { "HEAD".to_string() } else { end_version.to_string() };
                (range, if end_version.is_empty() { "Unreleased" } else { end_version }.to_string())
            }
        }
    } else {
        let range = format!("{}..{}", start_version, if end_version.is_empty() { "HEAD" } else { end_version });
        (range, end_version.to_string())
    };

    println!("{}", colors::yellow(&format!("预览范围: {}", range)));
    println!("{}", colors::yellow(&format!("仓库地址: {}", get_repo_url_cached())));
    println!("{}", colors::yellow("========================================"));

    let content = generate_version_content(&version, &range);
    print!("{}", content);
}

fn generate_changelog(start_version: &str, end_version: &str, force: bool) {
    let (range, new_version) = if start_version.is_empty() {
        match git_command(&["describe", "--tags", "--abbrev=0"]) {
            Ok(latest_tag) => {
                let range = format!("{}..{}", latest_tag, if end_version.is_empty() { "HEAD" } else { end_version });
                println!("{}", colors::yellow(&format!("使用最新 tag: {}", latest_tag)));
                (range, latest_tag)
            }
            Err(_) => {
                let range = if end_version.is_empty() { "HEAD".to_string() } else { end_version.to_string() };
                println!("{}", colors::yellow("未找到 tag，从第一个 commit 开始"));
                (range, if end_version.is_empty() { "Unreleased" } else { end_version }.to_string())
            }
        }
    } else {
        let range = format!("{}..{}", start_version, if end_version.is_empty() { "HEAD" } else { end_version });
        (range, end_version.to_string())
    };

    // 跳过 Unreleased
    if new_version == "Unreleased" {
        println!("{}", colors::yellow("跳过生成 Unreleased 版本"));
        return;
    }

    // 检查版本是否存在
    let existing_versions = get_existing_versions();
    if !force && existing_versions.contains(&new_version) {
        println!("{}", colors::red(&format!("版本 {} 已存在于 {} 中", new_version, CHANGELOG_FILE)));
        println!("{}", colors::yellow("使用 -f 选项强制覆盖"));
        return;
    }

    println!("{}", colors::green("正在生成 CHANGELOG 条目..."));
    println!("{}", colors::yellow(&format!("范围: {}", range)));
    println!("{}", colors::yellow(&format!("仓库地址: {}", get_repo_url_cached())));

    let new_content = generate_version_content(&new_version, &range);

    if new_content.is_empty() {
        println!("{}", colors::red("生成失败"));
        return;
    }

    if let Err(e) = update_changelog(&new_content) {
        println!("{}", colors::red(&format!("更新失败: {}", e)));
        return;
    }

    println!("{}", colors::green("完成！"));
}

fn main() {
    if !check_git_repo() {
        eprintln!("{}", colors::red("错误: 当前目录不是 Git 仓库"));
        std::process::exit(1);
    }

    let options = parse_args();

    if options.preview {
        preview_changelog(&options.start_version, &options.end_version);
    } else {
        generate_changelog(&options.start_version, &options.end_version, options.force);
    }
}
