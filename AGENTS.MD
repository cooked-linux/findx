# AGENTS.md — Developer & AI Agent Guidelines for findx

Welcome to `findx`. This document serves as the primary operational blueprint, architectural reference, and coding guide for both human developers and autonomous AI agents working within this repository.

---

## 1. Project Overview & Mission

`findx` is a high-performance, clean-room reverse-engineered implementation of the macOS **Finder** and **Spotlight** desktop experience, built natively in **Rust (2024 Edition)** for modern Linux desktop environments (Wayland-first with X11 compatibility).

### Core Goals
- **macOS Polish on Linux:** Deliver the fluid navigation, instantaneous response, keyboard-driven ergonomics, and visual elegance of macOS Finder and Spotlight.
- **Native Linux Primitives:** Map all proprietary macOS frameworks (`DesktopServicesPriv`, `Metadata.framework`, `QuickLook.framework`, `Calculate.framework`) cleanly to Linux standards: Wayland layer-shell, XDG Desktop specifications, DBus, POSIX extended attributes, `inotify`/`fanotify`, `copy_file_range`, and Bubblewrap isolation.
- **Safety & Performance:** Zero garbage collection pauses, low memory footprint, deterministic resource cleanup, non-blocking asynchronous I/O, and hard deadlines for UI interactions (e.g., query ranking under 50ms).

---

## 2. Repository Layout & Architecture

The project is organized as a Cargo workspace with dedicated crates, modular design assets, specifications, and skill guidelines:

```text
findx/
├── Cargo.toml                    # Workspace manifest (members: finder, spotlight, shared)
├── Cargo.lock                    # Dependency lockfile
├── AGENTS.MD                     # This operating guide
├── .agents/                      # Skills & AI tooling configurations
│   └── skills/rust-skills/       # Rust guideline skill (265 rules across 26 categories)
├── .codegraph/                   # CodeGraph symbol index
├── design/                       # UI prototypes, interactive HTML, and visual references
│   ├── finder/                   # Finder interactive HTML prototype & mockups
│   │   ├── Finder-style File Manager.html
│   │   ├── Columns (light)@1x.png
│   │   ├── Icon grid (light)@1x.png
│   │   ├── Nested list (dark)@1x.png
│   │   ├── Quick Look (dark)@1x.png
│   │   ├── Conflict sheet + queue (light)@1x.png
│   │   └── Search + Smart Folder (light)@1x.png
│   └── spotlight/                # Spotlight interactive HTML prototype & mockups
│       ├── Spotlight-style Launcher.html
│       ├── Calculator@1x.png
│       ├── Conversion@1x.png
│       ├── Definition@1x.png
│       ├── Typing app results@1x.png
│       ├── Mixed results with file preview@1x.png
│       ├── Empty state@1x.png
│       └── No results@1x.png
├── specs/                        # 29 Clean-room reverse-engineered feature specifications
│   ├── finder/                   # 21 Finder specifications + INDEX.md
│   │   ├── INDEX.md
│   │   ├── SPEC_MILLER_COLUMNS_VIEW.md
│   │   ├── SPEC_LIST_VIEW_TREE_EXPANSION.md
│   │   ├── SPEC_BREADCRUMBS_PATH_BAR.md
│   │   ├── SPEC_SIDEBAR_NAVIGATION_TREE.md
│   │   ├── SPEC_FILE_TAGGING_AND_LABELS.md
│   │   ├── SPEC_EXTENDED_ATTRIBUTES_AND_COMMENTS.md
│   │   ├── SPEC_BATCH_FILE_OPERATIONS_QUEUE.md
│   │   ├── SPEC_CONFLICT_RESOLUTION_ENGINE.md
│   │   ├── SPEC_ASYNC_FOLDER_SIZE_CALCULATION.md
│   │   ├── SPEC_UNDO_REDO_OPERATIONS_ENGINE.md
│   │   ├── SPEC_QUICK_LOOK_INSTANT_PREVIEWS.md
│   │   ├── SPEC_THUMBNAIL_GENERATION_PIPELINE.md
│   │   ├── SPEC_METADATA_SUMMARY_INSPECTOR.md
│   │   ├── SPEC_SMART_FOLDERS_SAVED_SEARCH.md
│   │   ├── SPEC_SEARCH_AND_METADATA_INDEXING.md
│   │   ├── SPEC_DYNAMIC_GROUPING_AND_SORTING.md
│   │   ├── SPEC_MIME_ROUTING_AND_DEFAULT_APPS.md
│   │   ├── SPEC_OPEN_WITH_MENU_DISPATCH.md
│   │   ├── SPEC_EXECUTABLE_PERMISSIONS_AND_EXECUTION.md
│   │   ├── SPEC_MULTI_FILE_ARCHIVE_CREATION.md
│   │   └── SPEC_ARCHIVE_EXTRACTION_AND_INTEGRITY.md
│   └── spotlight/                # 8 Spotlight specifications + README.md
│       ├── README.md
│       ├── SPEC_GLOBAL_OVERLAY_WINDOW.md
│       ├── SPEC_APPLICATION_LAUNCHER.md
│       ├── SPEC_FILE_CONTENT_INDEXING.md
│       ├── SPEC_INLINE_CALCULATOR_CONVERTER.md
│       ├── SPEC_DICTIONARY_QUICK_ANSWERS.md
│       ├── SPEC_PREVIEW_METADATA_INSPECTOR.md
│       ├── SPEC_SYSTEM_ACTIONS_WEB_FALLBACK.md
│       └── SPEC_QUERY_DISPATCH_AND_RANKING_ARBITER.md
├── finder/                       # Binary crate: macOS-style File Manager
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── spotlight/                    # Binary crate: macOS-style Search & Action Overlay
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
└── shared/                       # Library crate: Shared domain models, DB, IPC & utilities
    ├── Cargo.toml
    └── src/
        └── lib.rs
```

---

## 3. Workspace Crate Division of Responsibilities

### `finder` (Binary)
- **Primary Role:** Graphical file manager application.
- **Responsibilities:**
  - Multi-column browsing (Miller columns), recursive list views with disclosure arrows, icon grid views.
  - Sidebar with pinned places, bookmarks, and UDisks2-monitored volumes.
  - Breadcrumbs path bar with popover navigation and drop-targets.
  - Quick Look instant preview window (Spacebar trigger) with out-of-process Bubblewrap sandboxing.
  - Asynchronous batch file operations engine (copy, move, delete, trash) with `copy_file_range` reflink cloning.
  - Conflict resolution sheets (Replace, Keep Both, Merge, Skip).
  - Multi-level transactional undo/redo stack (`Ctrl+Z`, `Ctrl+Shift+Z`).
  - Archive creation and extraction (ZIP, tar, gzip, zstd) with Zip-Slip path-traversal prevention.

### `spotlight` (Binary)
- **Primary Role:** Global floating overlay and launcher (`Ctrl+Space` / `Super+Space`).
- **Responsibilities:**
  - Borderless, frosted-glass floating overlay window positioned via Wayland `wlr-layer-shell` or X11 splash window.
  - Fast query dispatch and ranking arbiter with micro-debouncing (<50ms response deadline).
  - XDG `.desktop` application indexer with time-decayed frecency algorithm.
  - Real-time inline calculator and multi-category dimensional unit converter.
  - Offline lexical dictionary and thesaurus quick answers.
  - Fast file search using embedded SQLite FTS5 index.
  - Right-hand preview inspector pane (syntax highlighting, PDF previews, media waveform/scrubber, EXIF metadata).
  - System control actions (Sleep, Lock, Restart, Shutdown, Empty Trash, volume/brightness controls) via `systemd-logind` and DBus.

### `shared` (Library)
- **Primary Role:** Common foundation and shared primitives across `finder` and `spotlight`.
- **Responsibilities:**
  - Domain models (file metadata, MIME detection, search records, preview payloads, tags).
  - Filesystem utilities: POSIX extended attributes (`user.xdg.tags`, `user.xdg.comment`), Freedesktop Trash specification (`~/.local/share/Trash`).
  - Freedesktop Thumbnail Cache specification v0.8 (`~/.cache/thumbnails/`).
  - SQLite FTS5 database schemas, migration runners, and query builders.
  - IPC protocols and D-Bus client wrappers (`org.freedesktop.FileManager1`, `org.freedesktop.portal.GlobalShortcuts`, `org.freedesktop.login1`).
  - Shared math and unit conversion wrappers.

---

## 4. Key Architectural Mappings (macOS to Linux)

| macOS Component / Feature | Linux Native Equivalent / Implementation Strategy |
|---|---|
| `NSPanel` / `kCGStatusWindowLevel` | Wayland `wlr-layer-shell` (`ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY`) / X11 `_NET_WM_WINDOW_TYPE_SPLASH` |
| `Carbon` Global Shortcuts / Event Tap | `xdg-desktop-portal` (`org.freedesktop.portal.GlobalShortcuts`) / X11 `XGrabKey` |
| `NSVisualEffectView` (frosted blur) | Compositor blur (`org_kde_kwin_blur`, Hyprland surface blur) / GTK4 CSS `backdrop-filter` |
| LaunchServices (`.app` bundles) | Freedesktop Desktop Entry Specification v1.5 (`/usr/share/applications`, `~/.local/share/applications`) |
| CoreSpotlight Frecency Engine | SQLite-backed table with 4-day exponential half-life decay formula |
| `mds` / `mdworker` daemon pool | `fanotify` / `inotify` file watchers + worker threads sandboxed via Bubblewrap (`bwrap`) |
| `/.Spotlight-V100/Store.db` | Embedded SQLite 3 with FTS5 virtual tables, WAL mode, unicode61 tokenizer |
| `Calculate.framework` | `libqalculate` / high-precision AST-based math and conversion engine |
| `Lookup.framework` | StarDict format (`sdcv`) / DICT protocol / embedded SQLite dictionary |
| `QuickLook.framework` (`quicklookd`) | Out-of-process sandboxed preview renderer + GtkSourceView5 / Poppler / ImageMagick |
| APFS clonefile / copyfile | Linux `copy_file_range` syscall (reflink copy on Btrfs/XFS/ZFS) with fallback |
| macOS Finder Tags / OpenMeta | `user.xdg.tags` and `user.xdg.color` extended attributes + SQLite tag cache |
| Finder Get Info Comments | `user.xdg.comment` extended attribute |
| Apple Events Finder Scripting | D-Bus interface `org.freedesktop.FileManager1.ShowItems` / `ShowFolders` |
| Finder Undo/Redo Manager | In-memory transactional ring buffer with serialized journals and Freedesktop Trash integration |

---

## 5. Technology Stack & Crate Ecosystem

According to the clean-room specifications suite, the system uses the following stacks, crates, and system libraries:

### 5.1 Architecture & Core Frameworks
| Layer | Technology Choice | Rationale from Specs |
|---|---|---|
| **Language & Toolchain** | **Rust (2024 Edition)** | Memory safety, zero-cost abstractions, deterministic low latency, and zero GC pauses. |
| **GUI Framework** | **GTK 4 + libadwaita** | Modern Freedesktop toolkit: `GtkListView`, `GtkColumnView` (Miller columns), `AdwPreferencesWindow`, `AdwActionRow`. |
| **Window Layering** | **`gtk4-layer-shell` / Wayland `wlr-layer-shell`** | Wayland surface placement for Spotlight floating overlay, keyboard capture, and screen anchoring. |
| **Async & Parallelism** | **`tokio` + `rayon`** | `tokio` for async I/O and D-Bus signals; `rayon` for parallel CPU workloads (directory sizing, indexing). |
| **Database & Indexing** | **Embedded SQLite 3 (FTS5 + WAL)** | Full-text content index, frecency engagement tracking, tag indices, and offline dictionary store. |
| **IPC & Portals** | **D-Bus (`zbus`)** | Native communication with `systemd-logind`, `FileManager1`, `UDisks2`, and `GlobalShortcuts` portal. |
| **Process Sandboxing** | **Bubblewrap (`bwrap`)** | Out-of-process preview and metadata extraction isolation to protect against parser exploits. |
| **Archive Engine** | **`libarchive`** | Multi-format archive creation (ZIP64) and safe extraction with Zip-Slip path-traversal prevention. |

### 5.2 Crate Ecosystem by Functional Domain

#### A. Core Runtime, Async & Utilities
- **`tokio`** (`features = ["full"]`): Primary async runtime for non-blocking I/O, timers, async process management, and thread pooling.
- **`rayon`**: Work-stealing parallel data processing (folder size calculation, concurrent file scanning).
- **`thiserror`**: Strongly typed domain errors for the `shared` crate.
- **`anyhow`**: Context-rich error propagation for application entry points (`finder`, `spotlight`).
- **`serde`** & **`serde_json`**: Configuration, action maps, and provider settings serialization.
- **`tracing`** & **`tracing-subscriber`**: Structured logging and performance profiling.

#### B. GUI & Window Management
- **`gtk4`** (`gtk-rs`): Primary UI toolkit bindings.
- **`libadwaita`**: Modern desktop widgets (`AdwPreferencesWindow`, `AdwActionRow`, `AdwMessageDialog`).
- **`gtk4-layer-shell`**: Wayland layer-shell protocol bindings for Spotlight's overlay window.
- **`x11rb`** or **`gdk4-x11`**: X11 fallback for global shortcut grab (`XGrabKey`) and splash window hints.

#### C. Filesystem, POSIX APIs & Batch Operations
- **`nix`** / **`libc`**: Low-level POSIX and Linux-specific syscalls:
  - `copy_file_range` (fast reflink cloning on Btrfs/XFS/ZFS)
  - `statx` (file birth time `stx_btime`, inode tracking)
  - `ioprio_set` (background I/O scheduling priority)
- **`xattr`**: Portable Linux extended attributes read/write (`user.xdg.tags`, `user.xdg.comment`, `user.xdg.origin.url`).
- **`notify`** or **`inotify`**: Real-time directory monitoring (`IN_CREATE`, `IN_DELETE`, `IN_MOVED_TO`).
- **`walkdir`** or **`jwalk`**: Fast, parallel directory tree traversal with cycle detection.
- **`trash`**: Freedesktop Trash standard implementation (`~/.local/share/Trash`).
- **`xdg`**: Resolving standard XDG base directory paths.

#### D. Desktop Integration, Applications & D-Bus (IPC)
- **`zbus`** (`features = ["tokio"]`): Asynchronous, type-safe D-Bus client and server:
  - `org.freedesktop.portal.GlobalShortcuts` (global activation shortcut `Ctrl+Space`)
  - `org.freedesktop.FileManager1` (`ShowItems`, `ShowFolders`)
  - `org.freedesktop.login1` (Lock, Suspend, Reboot, PowerOff via `systemd-logind`)
  - `org.freedesktop.UDisks2` (Volume monitoring and unmounting)
- **`freedesktop-desktop-entry`**: Fast parsing of `/usr/share/applications/*.desktop` entries.
- **`freedesktop-icons`** or **`linicon`**: XDG icon theme resolution across standard directories.
- **`mime_guess`**: File content type identification adhering to `shared-mime-info`.

#### E. Database, Indexing & Ranking
- **`rusqlite`** (`features = ["bundled", "vtab"]`): SQLite 3 with **FTS5** (Full-Text Search) and WAL mode.
- **`fuzzy-matcher`** or **`nucleo-matcher`**: Sub-millisecond fuzzy acronym and prefix matching for launcher queries.

#### F. Quick Look, Asset Previews & Metadata Inspection
- **`syntect`** or **`tree-sitter`**: Sub-millisecond syntax highlighting for code files in Quick Look and inspectors.
- **`image`**: Thumbnail scaling, rotation, and format conversion.
- **`kamadak-exif`** / **`rexif`**: EXIF photo metadata extraction (aperture, ISO, focal length, camera model).
- **`poppler-rs`** (or `poppler-glib` bindings): High-fidelity PDF page rendering to Cairo surface.
- **`gstreamer`** (`gstreamer-rs`): Audio/video preview playback, duration, and waveform extraction.

#### G. Archives & Compression
- **`compress-tools`** or raw **`libarchive`** bindings: High-performance streaming creation/extraction of ZIP, tar, gzip, bzip2, xz, and zstd archives with Zip-Slip path-traversal prevention.
- **`zip`**: Pure-Rust fallback for basic ZIP handling.

#### H. Spotlight Calculator, Converter & Dictionary
- **`qalculate` bindings** or **`evalexpr`** / **`muparserx`**: Math expressions evaluation and dimensional unit conversion.
- **`rust_decimal`** / **`num-bigint`**: Precision math without floating-point inaccuracies.
- **`rust-stemmers`** (Snowball stemmer): Morphological lemmatization and word root resolution for the dictionary provider.
- **`reqwest`**: Daily currency exchange rates ingestion from the European Central Bank (ECB).

### 5.3 Crate Distribution across Workspace Manifests

```toml
# shared/Cargo.toml
[dependencies]
tokio = { version = "1.43", features = ["sync", "fs", "io-util", "process", "rt-multi-thread"] }
rayon = "1.10"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
tracing = "0.1"
xdg = "2.5"
mime_guess = "2.0"
xattr = "1.4"
nix = { version = "0.29", features = ["fs", "sched", "process"] }
trash = "5.2"
rusqlite = { version = "0.33", features = ["bundled", "vtab"] }
zbus = { version = "5.5", features = ["tokio"] }
fuzzy-matcher = "0.3"

# finder/Cargo.toml
[dependencies]
shared = { path = "../shared" }
gtk4 = "0.9"
libadwaita = "0.7"
anyhow = "1.0"
notify = "8.0"
walkdir = "2.5"
compress-tools = "0.15"
kamadak-exif = "0.6"
image = "0.25"

# spotlight/Cargo.toml
[dependencies]
shared = { path = "../shared" }
gtk4 = "0.9"
libadwaita = "0.7"
gtk4-layer-shell = "0.4"
anyhow = "1.0"
freedesktop-desktop-entry = "0.1"
freedesktop-icons = "0.3"
syntect = "5.2"
rust-stemmers = "1.2"
evalexpr = "11.3"
```

### 5.4 Linux System Package Requirements

```bash
# Debian / Ubuntu
sudo apt install -y \
  build-essential \
  libgtk-4-dev \
  libadwaita-1-dev \
  libgtk4-layer-shell-dev \
  libsqlite3-dev \
  libarchive-dev \
  libpoppler-glib-dev \
  libgstreamer1.0-dev \
  libgstreamer-plugins-base1.0-dev \
  bubblewrap \
  shared-mime-info

# Arch Linux
sudo pacman -S --needed \
  base-devel \
  gtk4 \
  libadwaita \
  gtk4-layer-shell \
  sqlite \
  libarchive \
  poppler-glib \
  gstreamer \
  gst-plugins-base \
  bubblewrap \
  shared-mime-info
```

---

## 6. Development & Verification Commands

All agents and contributors should verify changes using the standard Cargo toolchain:

### Build & Check
```bash
# Check syntax and types across all workspace crates
cargo check --workspace

# Build all workspace binaries and libraries in debug mode
cargo build --workspace

# Build optimized release binaries
cargo build --workspace --release
```

### Testing & Verification
```bash
# Run unit and integration tests across the workspace
cargo test --workspace

# Run tests for a specific crate
cargo test -p shared
cargo test -p finder
cargo test -p spotlight
```

### Code Quality & Linting
```bash
# Check formatting
cargo fmt --all -- --check

# Format code in-place
cargo fmt --all

# Run Clippy with warnings treated as errors
cargo clippy --workspace --all-targets -- -D warnings
```

### Running Applications
```bash
# Launch Finder
cargo run -p finder

# Launch Spotlight overlay
cargo run -p spotlight
```

---

## 7. Coding Standards & Agent Guidelines

When generating, modifying, or reviewing code in this repository, all agents **must** adhere to the following principles:

### 1. CodeGraph First
When exploring symbol relationships, call hierarchies, or architectural flow in this repository, reach for CodeGraph **before** using grep, find, or reading unstructured files:
- Use the MCP tool `codegraph_explore` (or shell `codegraph explore "<query>"`) to retrieve exact source definitions and dependency call paths in a single call.

### 2. Rust Best Practices (`.agents/skills/rust-skills/`)
The repository includes a curated set of 265 Rust guidelines in `.agents/skills/rust-skills/SKILL.md`. Always observe the following critical priorities:
- **Error Handling (`err-`):** 
  - Never call `.unwrap()` or `.expect()` in production library or UI code; use `?` propagation with domain-specific errors created via `thiserror` (for libraries) or `anyhow` (for application entry points).
  - Preserve context when bubbling up I/O or filesystem errors.
- **Ownership & Memory (`own-`, `mem-`):** 
  - Prefer borrowed slices (`&str`, `&[T]`, `&Path`) over owned types in function parameters unless ownership transfer is required.
  - Avoid redundant `.clone()` calls; use references, `Cow<'a, T>`, or `Arc<T>` for shared state across async tasks.
  - Pre-allocate collection capacities with `Vec::with_capacity` when size is known or estimable.
- **Concurrency & Async (`conc-`, `async-`):** 
  - Keep UI threads responsive: offload blocking filesystem I/O, SQLite queries, and thumbnail generation to worker threads or asynchronous background tasks (`tokio::task::spawn_blocking`).
  - Ensure async cancellation safety: avoid holding locks across `.await` points.
- **Numeric & Conversions (`num-`, `conv-`):** 
  - Use checked or saturating arithmetic when computing file offsets, column pixel sizes, or zoom factors.
  - Implement standard conversion traits (`From`, `TryFrom`) rather than ad-hoc conversion methods.

### 3. Specification Compliance
Every feature implemented in `finder/` or `spotlight/` must strictly implement the behavioral and architectural contracts specified in `specs/finder/` and `specs/spotlight/`:
- Check the relevant `SPEC_*.md` file before designing struct layouts, state machines, or algorithmic heuristics.
- Respect performance budgets stated in specs (e.g., <50ms query arbitration deadline in Spotlight, smooth horizontal scrolling and debounced inotify cache invalidation in Finder).

### 4. Design Prototype Fidelity
Refer to the interactive prototypes in `design/` for visual hierarchy, layout constraints, color palettes, keyboard shortcut maps, and interaction states:
- Verify that keyboard navigation schemes match the specifications and mockups (e.g., arrow navigation, Tab focus traversal, Spacebar Quick Look trigger, Return to open, Cmd/Ctrl+O, Cmd/Ctrl+I).
- Support all specified UI states: empty queries, typing states, loading indicators, conflict sheets, and fallback displays.

### 5. Preservation of Documentation
- Maintain documentation integrity: preserve existing comments, docstrings, and module-level documentation unless intentionally refactoring them.
- Document public structs, traits, enums, and functions with clear docstrings explaining their invariants and error conditions.

---

## 8. Specification Reference Directory

### Finder Specifications (`specs/finder/`)
1. [`SPEC_MILLER_COLUMNS_VIEW.md`](specs/finder/SPEC_MILLER_COLUMNS_VIEW.md) — Cascading horizontal columns, selection syncing, terminal preview column.
2. [`SPEC_LIST_VIEW_TREE_EXPANSION.md`](specs/finder/SPEC_LIST_VIEW_TREE_EXPANSION.md) — Hierarchical tabular view, disclosure arrows, lazy expansion.
3. [`SPEC_BREADCRUMBS_PATH_BAR.md`](specs/finder/SPEC_BREADCRUMBS_PATH_BAR.md) — Path bar navigation, folder dropdown popovers, drag/drop target.
4. [`SPEC_SIDEBAR_NAVIGATION_TREE.md`](specs/finder/SPEC_SIDEBAR_NAVIGATION_TREE.md) — Pinned places, UDisks2 volume monitoring, 1-click eject.
5. [`SPEC_FILE_TAGGING_AND_LABELS.md`](specs/finder/SPEC_FILE_TAGGING_AND_LABELS.md) — 7 color labels, custom tags, `user.xdg.tags`, SQLite index.
6. [`SPEC_EXTENDED_ATTRIBUTES_AND_COMMENTS.md`](specs/finder/SPEC_EXTENDED_ATTRIBUTES_AND_COMMENTS.md) — `user.xdg.comment`, download origin tracking.
7. [`SPEC_BATCH_FILE_OPERATIONS_QUEUE.md`](specs/finder/SPEC_BATCH_FILE_OPERATIONS_QUEUE.md) — Pre-flight scanning, `copy_file_range` reflink, rate estimation.
8. [`SPEC_CONFLICT_RESOLUTION_ENGINE.md`](specs/finder/SPEC_CONFLICT_RESOLUTION_ENGINE.md) — File collision dialogs (Replace, Keep Both, Merge, Skip).
9. [`SPEC_ASYNC_FOLDER_SIZE_CALCULATION.md`](specs/finder/SPEC_ASYNC_FOLDER_SIZE_CALCULATION.md) — Non-blocking recursive directory size scan with inotify cache invalidation.
10. [`SPEC_UNDO_REDO_OPERATIONS_ENGINE.md`](specs/finder/SPEC_UNDO_REDO_OPERATIONS_ENGINE.md) — Multi-step transactional operation history, Trash restore.
11. [`SPEC_QUICK_LOOK_INSTANT_PREVIEWS.md`](specs/finder/SPEC_QUICK_LOOK_INSTANT_PREVIEWS.md) — Spacebar preview overlay, Bubblewrap process isolation.
12. [`SPEC_THUMBNAIL_GENERATION_PIPELINE.md`](specs/finder/SPEC_THUMBNAIL_GENERATION_PIPELINE.md) — Freedesktop thumbnail caching (`~/.cache/thumbnails/`).
13. [`SPEC_METADATA_SUMMARY_INSPECTOR.md`](specs/finder/SPEC_METADATA_SUMMARY_INSPECTOR.md) — "Get Info" dialog, EXIF/audio/video metadata sheets.
14. [`SPEC_SMART_FOLDERS_SAVED_SEARCH.md`](specs/finder/SPEC_SMART_FOLDERS_SAVED_SEARCH.md) — Dynamic virtual `.savedSearch` folders with boolean filters.
15. [`SPEC_SEARCH_AND_METADATA_INDEXING.md`](specs/finder/SPEC_SEARCH_AND_METADATA_INDEXING.md) — Live search filtering, scope toggles (This PC vs Current Folder).
16. [`SPEC_DYNAMIC_GROUPING_AND_SORTING.md`](specs/finder/SPEC_DYNAMIC_GROUPING_AND_SORTING.md) — Categorical grouping (Kind, Date, Size), sticky headers.
17. [`SPEC_MIME_ROUTING_AND_DEFAULT_APPS.md`](specs/finder/SPEC_MIME_ROUTING_AND_DEFAULT_APPS.md) — Shared-MIME-Info sniffing, `mimeapps.list` overrides.
18. [`SPEC_OPEN_WITH_MENU_DISPATCH.md`](specs/finder/SPEC_OPEN_WITH_MENU_DISPATCH.md) — Candidate app resolution, default association picker.
19. [`SPEC_EXECUTABLE_PERMISSIONS_AND_EXECUTION.md`](specs/finder/SPEC_EXECUTABLE_PERMISSIONS_AND_EXECUTION.md) — Script verification ("Run in Terminal" vs "Display").
20. [`SPEC_MULTI_FILE_ARCHIVE_CREATION.md`](specs/finder/SPEC_MULTI_FILE_ARCHIVE_CREATION.md) — Multi-file compression (`libarchive`), smart naming.
21. [`SPEC_ARCHIVE_EXTRACTION_AND_INTEGRITY.md`](specs/finder/SPEC_ARCHIVE_EXTRACTION_AND_INTEGRITY.md) — Safe extraction, Zip-Slip prevention, tar-bomb unnesting.

### Spotlight Specifications (`specs/spotlight/`)
1. [`SPEC_GLOBAL_OVERLAY_WINDOW.md`](specs/spotlight/SPEC_GLOBAL_OVERLAY_WINDOW.md) — Wayland `wlr-layer-shell` floating window, frosted blur, focus management.
2. [`SPEC_APPLICATION_LAUNCHER.md`](specs/spotlight/SPEC_APPLICATION_LAUNCHER.md) — XDG `.desktop` ingestion, icon caching, fuzzy acronym search, frecency.
3. [`SPEC_FILE_CONTENT_INDEXING.md`](specs/spotlight/SPEC_FILE_CONTENT_INDEXING.md) — `inotify`/`fanotify` filesystem crawler, SQLite FTS5 content search.
4. [`SPEC_INLINE_CALCULATOR_CONVERTER.md`](specs/spotlight/SPEC_INLINE_CALCULATOR_CONVERTER.md) — Math AST evaluation, dimensional unit conversion, ECB currency rates.
5. [`SPEC_DICTIONARY_QUICK_ANSWERS.md`](specs/spotlight/SPEC_DICTIONARY_QUICK_ANSWERS.md) — Offline dictionary lookups, IPA phonetics, synonyms.
6. [`SPEC_PREVIEW_METADATA_INSPECTOR.md`](specs/spotlight/SPEC_PREVIEW_METADATA_INSPECTOR.md) — Right-hand split-pane inspector, code viewer, PDF & media preview.
7. [`SPEC_SYSTEM_ACTIONS_WEB_FALLBACK.md`](specs/spotlight/SPEC_SYSTEM_ACTIONS_WEB_FALLBACK.md) — Power/session controls via `systemd-logind`, web search fallback.
8. [`SPEC_QUERY_DISPATCH_AND_RANKING_ARBITER.md`](specs/spotlight/SPEC_QUERY_DISPATCH_AND_RANKING_ARBITER.md) — Multi-provider concurrent fan-out, <50ms ranking arbiter.
