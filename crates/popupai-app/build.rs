use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("io.github.jeremiznoo.popupai").qml_files([
        "qml/Main.qml",
        "qml/PopupWindow.qml",
        "qml/LayerPopupWindow.qml",
    ]))
    .file("src/controller.rs")
    .build();
}
