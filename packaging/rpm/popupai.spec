# Built from a source tarball made with `git archive` (see .github/workflows/release.yml).
# Needs network access during %build: cargo fetches the Rust crates listed in Cargo.lock.
%global debug_package %{nil}
%global appid io.github.jeremiznoo.PopUpAI

Name:           popupai
Version:        0.0.1
Release:        1%{?dist}
Summary:        Quick-ask AI popup with a global shortcut
License:        MIT
URL:            https://github.com/Jeremiznoo/PopUpAI
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust
BuildRequires:  cargo
BuildRequires:  gcc-c++
BuildRequires:  cmake
BuildRequires:  qt6-qtbase-devel
BuildRequires:  qt6-qtdeclarative-devel
BuildRequires:  desktop-file-utils
Requires:       qt6-qtdeclarative
# The popup is a layer-shell surface on Wayland: this provides the org.kde.layershell QML module.
Requires:       layer-shell-qt
Requires:       hicolor-icon-theme

%description
PopUpAI shows a small card in the corner of the screen when you press a shortcut: type a question, read the
streamed answer, close it. It works with a ChatGPT account, API keys or a local model, keeps keys in the
system secret store, and is driven entirely from the keyboard. It runs on demand and quits when closed.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked -p popupai-app

%install
install -Dm755 target/release/popupai %{buildroot}%{_bindir}/popupai
install -Dm644 packaging/%{appid}.desktop %{buildroot}%{_datadir}/applications/%{appid}.desktop
mkdir -p %{buildroot}%{_datadir}/icons
cp -a packaging/icons/hicolor %{buildroot}%{_datadir}/icons/

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{appid}.desktop

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/popupai
%{_datadir}/applications/%{appid}.desktop
%{_datadir}/icons/hicolor/*/apps/%{appid}.*

%changelog
* Tue Nov 03 2026 jeremiznoo <diaz.jeremy@protonmail.com> - 0.0.1-1
- First package
