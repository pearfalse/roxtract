build:
	cargo build --bin roxtract --features=cli

release:
	cargo build --release --bin roxtract --features=cli
