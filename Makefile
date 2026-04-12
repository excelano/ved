PREFIX ?= /usr/local

.PHONY: build install uninstall clean test

build:
	cargo build --release

test:
	cargo test

install:
	@test -f target/release/ved || { echo "Run 'make build' first."; exit 1; }
	install -d $(PREFIX)/bin
	install -m 755 target/release/ved $(PREFIX)/bin/ved

uninstall:
	rm -f $(PREFIX)/bin/ved

clean:
	cargo clean
