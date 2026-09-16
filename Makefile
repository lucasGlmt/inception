.PHONY: build-cli install-cli uninstall-cli build-lsp install-lsp uninstall-lsp

LUX_BIN := target/release/lux
LSP_BIN := target/release/lux-lsp
INSTALL_DIR := $(HOME)/.local/bin
INSTALLED_LUX := $(INSTALL_DIR)/lux
INSTALLED_LSP := $(INSTALL_DIR)/lux-lsp
ZSHRC := $(HOME)/.zshrc
ZSH_PATH_LINE := export PATH="$$HOME/.local/bin:$$PATH"

build-cli:
	cargo build --release -p lux-cli --bin lux

install-cli: build-cli
	mkdir -p "$(INSTALL_DIR)"
	install -m 755 "$(LUX_BIN)" "$(INSTALLED_LUX)"
	touch "$(ZSHRC)"
	@grep -Fqx '$(ZSH_PATH_LINE)' "$(ZSHRC)" || \
		printf '\n%s\n' '$(ZSH_PATH_LINE)' >> "$(ZSHRC)"
	@printf 'Installed lux at %s\n' "$(INSTALLED_LUX)"
	@printf 'Open a new zsh shell or run: source %s\n' "$(ZSHRC)"

uninstall-cli:
	rm -f "$(INSTALLED_LUX)"
	@printf 'Removed %s (the PATH entry in %s was kept).\n' "$(INSTALLED_LUX)" "$(ZSHRC)"

build-lsp:
	cargo build --release -p lux-lsp --bin lux-lsp

install-lsp: build-lsp
	mkdir -p "$(INSTALL_DIR)"
	install -m 755 "$(LSP_BIN)" "$(INSTALLED_LSP)"
	touch "$(ZSHRC)"
	@grep -Fqx '$(ZSH_PATH_LINE)' "$(ZSHRC)" || \
		printf '\n%s\n' '$(ZSH_PATH_LINE)' >> "$(ZSHRC)"
	@printf 'Installed lux-lsp at %s\n' "$(INSTALLED_LSP)"
	@printf 'Open a new zsh shell or run: source %s\n' "$(ZSHRC)"

uninstall-lsp:
	rm -f "$(INSTALLED_LSP)"
	@printf 'Removed %s (the PATH entry in %s was kept).\n' "$(INSTALLED_LSP)" "$(ZSHRC)"
