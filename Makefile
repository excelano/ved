PREFIX ?= /usr/local

.PHONY: build install uninstall clean test

build:
	cargo build --release

test:
	cargo test

install: build
	install -d $(PREFIX)/bin
	install -m 755 target/release/ved $(PREFIX)/bin/ved

uninstall:
	rm -f $(PREFIX)/bin/ved

clean:
	cargo clean
