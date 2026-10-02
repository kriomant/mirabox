# Install maturin in the project-local Python environment.
install-maturin:
    test -x python/.venv/bin/python || uv venv python/.venv
    uv pip install --python python/.venv/bin/python 'maturin>=1.8,<2'

# Build and install the Python module in editable mode.
install-module: install-maturin
    VIRTUAL_ENV=python/.venv python/.venv/bin/maturin develop --manifest-path python/Cargo.toml --locked
