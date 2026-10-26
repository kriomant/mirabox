# Install maturin in the project-local Python environment.
install-maturin:
    test -x mirabox-n4pro-py/.venv/bin/python || uv venv mirabox-n4pro-py/.venv
    uv pip install --python mirabox-n4pro-py/.venv/bin/python 'maturin>=1.8,<2'

# Build and install the Python module in editable mode.
install-module: install-maturin
    VIRTUAL_ENV=mirabox-n4pro-py/.venv mirabox-n4pro-py/.venv/bin/maturin develop --manifest-path mirabox-n4pro-py/Cargo.toml --locked
