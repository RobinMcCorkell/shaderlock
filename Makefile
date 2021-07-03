PREFIX = $(HOME)/.local
BINDIR = $(PREFIX)/bin
export DATADIR = $(PREFIX)/share/shaderlock
export PAM_SERVICE = system-auth

all: shaderlock.daemon dist
	cargo build --release

install: all
	install -D -t $(BINDIR) target/release/shaderlock shaderlock.daemon
	install -d $(DATADIR)
	cp -a -t $(DATADIR) dist/*

.PHONY: all install
