# Calibre OPDS Server

A lightweight OPDS server for Calibre libraries written in Rust.

## Features

- ✅ OPDS 1.2 compliant feeds
- ✅ Read-only access to Calibre metadata.db
- ✅ Support for custom columns
- ✅ Multiple format support (EPUB, PDF, MOBI, AZW3, etc.)
- ✅ Book cover images
- ✅ Pagination
- 🔒 Designed to run behind reverse proxy (nginx/caddy) for auth/TLS

## Configuration

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
cargo run --release
```

## OPDS Endpoints

- `GET /` - Root catalog
- `GET /books?page=1&per_page=50` - All books (paginated)
- `GET /download/:id/:format` - Download book (e.g., `/download/123/epub`)
- `GET /cover/:id` - Get book cover image

## Reverse Proxy Setup (nginx)

Example nginx configuration with HTTP Basic Auth:

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

## Custom Columns

All Calibre custom columns are automatically included in OPDS feeds under the `calibre:` namespace.

Example:
```xml
<entry>
  <title>Book Title</title>
  <calibre:rating>5</calibre:rating>
  <calibre:tags>fiction, scifi</calibre:tags>
  ...
</entry>
```

## Compatible Clients

- FBReader
- KOReader
- Calibre (OPDS browser)
- Moon+ Reader
- PocketBook readers
- Most OPDS-compatible e-readers

## License

MIT
