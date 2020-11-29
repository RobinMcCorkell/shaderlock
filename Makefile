prefix = $(HOME)/.local
bindir = $(prefix)/bin
distdir = $(prefix)/share/shaderlock

all: shaderlock.daemon dist
	cargo build --release

install: all
	install -D -t $(bindir) target/release/shaderlock shaderlock.daemon
	install -d $(distdir)
	cp -a -t $(distdir) dist/*

.PHONY: all install
