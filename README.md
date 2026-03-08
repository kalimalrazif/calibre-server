# Calibre OPDS Server

A lightweight, fast OPDS server for Calibre libraries written in Rust.

[![License: GPL v2](https://img.shields.io/badge/License-GPL%20v2-blue.svg)](https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

## Features

- ✅ **OPDS 1.2 compliant** - Full OPDS feed support with complete metadata
- ✅ **HTML web interface** - Browse your library from any web browser
- ✅ **Read-only access** - Safe access to Calibre metadata.db
- ✅ **Custom columns** - Full support for Calibre custom columns
- ✅ **Multiple formats** - EPUB, PDF, MOBI, AZW3, and more
- ✅ **EPUB metadata sync** - Automatically updates EPUB metadata on download
- ✅ **Book covers** - Display cover images in all views
- ✅ **Advanced search** - OpenSearch integration with full-text search
- ✅ **Smart navigation** - Browse by authors, series, tags, publishers, languages, ratings
- ✅ **Read tracking** - Special catalog for books read by year
- ✅ **Pagination** - Efficient handling of large libraries
- ✅ **Fast & lightweight** - Built with Rust for maximum performance
- 🔒 **Proxy-ready** - Designed to run behind nginx/caddy for auth/TLS

## Installation

### Prerequisites

- Rust 1.70 or later
- A Calibre library

### From Source

```bash
git clone git@github.com:kalimalrazif/calibre-server.git
cd calibre-server
cargo build --release
```

The binary will be available at `target/release/calibre-server`

## Usage

### Command Line Arguments

```bash
calibre-server --library /path/to/your/calibre/library --host 0.0.0.0 --port 8080
```

**Options:**
- `-l, --library <PATH>` - Path to Calibre library directory (required)
- `--host <HOST>` - Server host address (default: 127.0.0.1)
- `-p, --port <PORT>` - Server port (default: 8080)
- `-b, --base-url <URL>` - Base URL for OPDS links (optional, auto-detected)
- `-h, --help` - Print help
- `-V, --version` - Print version

### Environment Variables

All arguments can also be set via environment variables:

```bash
export CALIBRE_LIBRARY_PATH="/path/to/your/calibre/library"
export HOST="127.0.0.1"
export PORT="8080"
export BASE_URL="http://localhost:8080"
```

Command line arguments take precedence over environment variables.

## Running

```bash
cargo run --release -- --library /path/to/calibre/library --host 0.0.0.0 --port 8080
```

Or after building:

```bash
./target/release/calibre-server --library /path/to/calibre/library
```

## Accessing the Server

### HTML Interface

Open your browser and navigate to:
```
http://your-server:8080/html
```

Features:
- Browse books by various categories
- Search functionality
- Responsive design for mobile and desktop
- Direct download links for all formats
- Book covers in list views

### OPDS Feed

Configure your OPDS-compatible reader with:
```
http://your-server:8080/
```

## OPDS Endpoints

- `GET /` - Root catalog
- `GET /books?page=1&per_page=50` - All books (paginated)
- `GET /recent` - Recently added books
- `GET /authors` - Browse by authors
- `GET /series` - Browse by series
- `GET /tags` - Browse by tags
- `GET /publishers` - Browse by publishers
- `GET /languages` - Browse by languages
- `GET /ratings` - Browse by ratings
- `GET /read-years` - Books read by year
- `GET /search?q=query` - Search books
- `GET /download/:id/:format` - Download book
- `GET /cover/:id` - Get book cover image

## Reverse Proxy Setup

### Nginx Example

```nginx
server {
    listen 80;
    server_name books.example.com;

    # Basic auth
    auth_basic "Calibre Library";
    auth_basic_user_file /etc/nginx/.htpasswd;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Create password file:
```bash
htpasswd -c /etc/nginx/.htpasswd username
```

### Caddy Example

```caddy
books.example.com {
    basicauth {
        username $2a$14$hashed_password
    }
    reverse_proxy localhost:8080
}
```

## Custom Columns

All Calibre custom columns are automatically included in OPDS feeds under the `calibre:` namespace.

Example:
```xml
<entry>
  <title>Book Title</title>
  <calibre:rating>5</calibre:rating>
  <calibre:tags>fiction, scifi</calibre:tags>
  <calibre:leido>true</calibre:leido>
  ...
</entry>
```

## EPUB Metadata Update

When downloading EPUB files, the server automatically updates the metadata (title, author) with the current information from your Calibre database. This ensures your downloaded EPUBs always have the latest metadata even if you've made corrections in Calibre.

This feature only applies to EPUB files. Other formats (PDF, MOBI, AZW3) are served as-is.

## Compatible Clients

### OPDS Readers
- FBReader
- KOReader
- Calibre (OPDS browser)
- Moon+ Reader
- PocketBook readers
- Most OPDS-compatible e-readers

### Web Browsers
- Chrome/Chromium
- Firefox
- Safari
- Edge
- Mobile browsers

## Development

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Linting

```bash
cargo clippy
```

### Formatting

```bash
cargo fmt
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the GNU General Public License v2.0 - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with [Axum](https://github.com/tokio-rs/axum) web framework
- Uses [SQLx](https://github.com/launchbadge/sqlx) for database access
- Template rendering with [Askama](https://github.com/djc/askama)
- Inspired by [COPS](https://github.com/seblucas/cops)

## Author

Created by [@kalimalrazif](https://github.com/kalimalrazif)

## Support

If you encounter any issues or have questions, please [open an issue](https://github.com/kalimalrazif/calibre-server/issues) on GitHub.

