PREFIX = $(HOME)/.local
BINDIR = $(PREFIX)/bin
export DATADIR = $(PREFIX)/share/shaderlock

all: shaderlock.daemon dist
	cargo build --release

install: all
	install -D -t $(BINDIR) target/release/shaderlock shaderlock.daemon
	install -d $(DATADIR)
	cp -a -t $(DATADIR) dist/*

.PHONY: all install
