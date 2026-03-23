# 5-Spot Documentation

This directory contains the MkDocs-based documentation for 5-Spot.

## Quick Start

### Prerequisites

- Python 3.10+
- Poetry (recommended) or pip

### Setup

```bash
cd docs

# Using Poetry (recommended)
poetry install
poetry run mkdocs serve

# Or using pip
pip install -r requirements.txt
mkdocs serve
```

### Development

```bash
# Start local dev server with hot-reload
poetry run mkdocs serve

# Build static site
poetry run mkdocs build

# Deploy to GitHub Pages
poetry run mkdocs gh-deploy
```

## Structure

```
docs/
├── mkdocs.yml           # MkDocs configuration
├── pyproject.toml       # Python dependencies
├── .python-version      # Python version
├── .gitignore          # Git ignore rules
└── src/                # Documentation source
    ├── index.md        # Homepage
    ├── stylesheets/    # Custom CSS
    ├── javascripts/    # Custom JavaScript
    ├── images/         # Images and assets
    ├── installation/   # Installation guides
    ├── concepts/       # Core concepts
    ├── operations/     # Operations guides
    ├── advanced/       # Advanced topics
    ├── development/    # Developer guides
    ├── reference/      # API reference
    └── roadmaps/       # Project roadmaps
```

## Adding New Pages

1. Create a new `.md` file in the appropriate directory
2. Add the page to the `nav` section in `mkdocs.yml`
3. Run `mkdocs serve` to preview

## Features

- **Material Theme**: Modern, responsive design
- **Dark Mode**: Toggle between light and dark themes
- **Search**: Full-text search
- **Mermaid Diagrams**: Interactive diagrams
- **Code Highlighting**: Syntax highlighting for code blocks
- **Git Integration**: Last updated timestamps

## Contributing

See [Contributing Guide](src/development/contributing.md) for documentation contribution guidelines.
