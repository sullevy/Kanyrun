use crate::config::LoadedConfig;
use crate::menu::MenuModel;
use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub trait MenuPresenter {
    fn show_menu(&mut self, menu: &MenuModel) -> Result<Option<String>, String>;
}

pub enum DefaultPresenter {
    Kwin(KwinPresenter),
    Qt(QtPresenter),
}

pub struct QtPresenter {
    program: PathBuf,
    show_icons: bool,
}

impl DefaultPresenter {
    pub fn from_config(config: &LoadedConfig) -> Result<Self, String> {
        let show_icons = config
            .app
            .ui
            .as_ref()
            .and_then(|ui| ui.show_icons)
            .unwrap_or(true);

        match config.app.ui.as_ref().and_then(|ui| ui.frontend.as_deref()) {
            None | Some("kwin") => Ok(Self::Kwin(KwinPresenter::new(show_icons))),
            Some("qt") => Ok(Self::Qt(QtPresenter {
                program: find_ui_program("kanyrun-ui"),
                show_icons,
            })),
            Some(other) => Err(format!("unsupported ui frontend: {other}")),
        }
    }
}

impl MenuPresenter for DefaultPresenter {
    fn show_menu(&mut self, menu: &MenuModel) -> Result<Option<String>, String> {
        match self {
            Self::Kwin(presenter) => presenter.show_menu(menu),
            Self::Qt(presenter) => show_menu_with(menu, presenter.show_icons, &SystemUiProcessRunner, &presenter.program),
        }
    }
}

pub struct KwinPresenter {
    show_icons: bool,
}

impl KwinPresenter {
    fn new(show_icons: bool) -> Self {
        Self { show_icons }
    }

    fn show(&self, menu: &MenuModel) -> Result<Option<String>, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("failed to bind menu callback listener: {error}"))?;
        listener
            .set_nonblocking(false)
            .map_err(|error| format!("failed to configure menu callback listener: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("failed to read menu callback listener address: {error}"))?
            .port();
        let req_id = request_id();
        let qml = render_kwin_qml(menu, self.show_icons, port, &req_id)?;
        let qml_path = write_kwin_qml(&qml, &req_id)?;
        let plugin = "kanyrun-menu";

        let _ = unload_kwin_script(plugin);
        let load_result = load_kwin_script(&qml_path, plugin).and_then(|_| start_kwin_scripts());
        if let Err(error) = load_result {
            let _ = fs::remove_file(&qml_path);
            return Err(error);
        }

        let wait = thread::spawn(move || wait_for_selection(listener, &req_id, Duration::from_secs(120)));
        let result = wait.join().unwrap_or_else(|_| Ok(None));
        let _ = unload_kwin_script(plugin);
        let _ = fs::remove_file(&qml_path);
        result
    }
}

impl MenuPresenter for KwinPresenter {
    fn show_menu(&mut self, menu: &MenuModel) -> Result<Option<String>, String> {
        self.show(menu)
    }
}

fn request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("p{}t{}", std::process::id(), nanos)
}

fn render_kwin_qml(menu: &MenuModel, show_icons: bool, port: u16, req_id: &str) -> Result<String, String> {
    let payload = serialize_menu(menu, show_icons)?;
    Ok(KWIN_MENU_QML
        .replace("__MENU_JSON__", &qml_string_literal(&payload))
        .replace("__PORT__", &port.to_string())
        .replace("__REQ_ID__", req_id))
}

fn write_kwin_qml(contents: &str, req_id: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("kanyrun");
    fs::create_dir_all(&dir).map_err(|error| format!("failed to create {}: {error}", dir.display()))?;
    let path = dir.join(format!("menu-{req_id}.qml"));
    fs::write(&path, contents).map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(path)
}

fn qml_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"{}\"".into())
}

fn unload_kwin_script(plugin: &str) -> Result<(), String> {
    run_busctl(&["call", "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "unloadScript", "s", plugin])
}

fn load_kwin_script(path: &Path, plugin: &str) -> Result<(), String> {
    let path = path
        .to_str()
        .ok_or_else(|| format!("non-utf8 qml path: {}", path.display()))?;
    run_busctl(&[
        "call",
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
        "loadDeclarativeScript",
        "ss",
        path,
        plugin,
    ])
}

fn start_kwin_scripts() -> Result<(), String> {
    run_busctl(&["call", "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "start"])
}

fn run_busctl(args: &[&str]) -> Result<(), String> {
    let output = Command::new("busctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|error| format!("failed to run busctl: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("busctl exited with status {}", output.status)
        } else {
            format!("busctl exited with status {}: {stderr}", output.status)
        })
    }
}

fn wait_for_selection(listener: TcpListener, req_id: &str, timeout: Duration) -> Result<Option<String>, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("failed to configure callback listener: {error}"))?;
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => return read_selection(&mut stream, req_id),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= timeout {
                    return Ok(None);
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(format!("failed to accept menu callback: {error}")),
        }
    }
}

fn read_selection(stream: &mut TcpStream, req_id: &str) -> Result<Option<String>, String> {
    let mut request = [0_u8; 2048];
    let len = stream
        .read(&mut request)
        .map_err(|error| format!("failed to read menu callback: {error}"))?;
    let request = String::from_utf8_lossy(&request[..len]);
    let first_line = request.lines().next().unwrap_or_default();
    let id = parse_selection_request(first_line, req_id)?;
    let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 0\r\n\r\n");
    Ok(id.filter(|value| !value.is_empty()))
}

fn parse_selection_request(first_line: &str, req_id: &str) -> Result<Option<String>, String> {
    let path = first_line
        .strip_prefix("GET ")
        .and_then(|line| line.split_whitespace().next())
        .ok_or_else(|| "invalid menu callback request".to_string())?;
    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default();
    let mut seen_req = false;
    let mut selected = None;
    for part in query.split('&') {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        let value = percent_decode(value);
        match key {
            "req" if value == req_id => seen_req = true,
            "id" => selected = Some(value),
            _ => {}
        }
    }
    if seen_req {
        Ok(selected)
    } else {
        Err("menu callback request id mismatch".into())
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            decoded.push(high * 16 + low);
            i += 3;
            continue;
        }
        decoded.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

const KWIN_MENU_QML: &str = r##"
import QtQuick
import QtQuick.Window
import org.kde.kwin
import org.kde.plasma.core as PlasmaCore

Item {
    id: root

    property var payload: JSON.parse(__MENU_JSON__)
    property string reqId: "__REQ_ID__"
    property int callbackPort: __PORT__
    property var area: ({ x: 0, y: 0, width: 1, height: 1 })
    property int panelWidth: 220
    property int rowHeight: 28
    property int margin: 8
    property var panels: []
    property bool finished: false
    property bool readyToDismiss: false

    function request(path) {
        var xhr = new XMLHttpRequest();
        xhr.open("GET", "http://127.0.0.1:" + callbackPort + path);
        xhr.send();
    }

    function finish(id) {
        if (finished) return;
        finished = true;
        dialog.visible = false;
        request("/select?req=" + encodeURIComponent(reqId) + "&id=" + encodeURIComponent(id || ""));
    }

    function clamp(value, minValue, maxValue) {
        return Math.max(minValue, Math.min(maxValue, value));
    }

    function visiblePanelCount() {
        return panels.length;
    }

    function panelActions(index) {
        return panels[index] ? panels[index].actions : [];
    }

    function panelX(index) {
        var x = panels[index] ? panels[index].x : 0;
        return clamp(x, margin, Math.max(margin, area.width - panelWidth - margin));
    }

    function panelY(index) {
        var actions = panelActions(index);
        var h = Math.max(rowHeight, actions.length * rowHeight);
        var y = panels[index] ? panels[index].y : 0;
        return clamp(y, margin, Math.max(margin, area.height - h - margin));
    }

    function areaContainsPoint(a, point) {
        return point.x >= a.x && point.x < a.x + a.width && point.y >= a.y && point.y < a.y + a.height;
    }

    function outputForCursor(point) {
        if (typeof Workspace.screenAt === "function") {
            var screen = Workspace.screenAt(point);
            if (screen) {
                return screen;
            }
        }

        var active = Workspace.activeWindow;
        if (active) {
            var activeArea = Workspace.clientArea(KWin.MaximizeArea, active.output, Workspace.currentDesktop);
            if (areaContainsPoint(activeArea, point)) {
                return active.output;
            }
        }

        return Workspace.activeScreen;
    }

    function openRoot() {
        var cur = Workspace.cursorPos;
        var a = Workspace.clientArea(KWin.MaximizeArea, outputForCursor(cur), Workspace.currentDesktop);
        area = { x: a.x, y: a.y, width: a.width, height: a.height };
        panels = [{ actions: payload.actions || [], x: cur.x - a.x, y: cur.y - a.y }];
        repeater.model = panels.length;
        dialog.visible = true;
        dismissGuard.start();
    }

    function openSubmenu(panelIndex, rowIndex, item) {
        if (!item.submenu || item.submenu.length === 0) {
            panels = panels.slice(0, panelIndex + 1);
            repeater.model = panels.length;
            return;
        }
        var baseX = panelX(panelIndex) + panelWidth - 4;
        var baseY = panelY(panelIndex) + rowIndex * rowHeight;
        var next = panels.slice(0, panelIndex + 1);
        next.push({ actions: item.submenu, x: baseX, y: baseY });
        panels = next;
        repeater.model = panels.length;
    }

    Component.onCompleted: openRoot()

    Timer {
        id: dismissGuard
        interval: 120
        repeat: false
        onTriggered: root.readyToDismiss = true
    }

    PlasmaCore.Dialog {
        id: dialog
        x: root.area.x
        y: root.area.y
        width: root.area.width
        height: root.area.height
        flags: Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.WindowDoesNotAcceptFocus
        backgroundHints: PlasmaCore.Types.NoBackground
        location: PlasmaCore.Types.Floating
        visible: false
        type: PlasmaCore.Dialog.PopupMenu
        mainItem: Item {
            width: root.area.width
            height: root.area.height

            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                onPressed: function(mouse) {
                    if (!root.readyToDismiss) {
                        return;
                    }
                    if (mouse.button === Qt.RightButton) {
                        root.finish("");
                        return;
                    }
                    for (var i = 0; i < root.visiblePanelCount(); i++) {
                        var px = root.panelX(i);
                        var py = root.panelY(i);
                        var ph = Math.max(root.rowHeight, root.panelActions(i).length * root.rowHeight);
                        if (mouse.x >= px && mouse.x < px + root.panelWidth && mouse.y >= py && mouse.y < py + ph) {
                            mouse.accepted = false;
                            return;
                        }
                    }
                    root.finish("");
                }
            }

            Repeater {
                id: repeater
                model: 0

                delegate: Rectangle {
                    id: panel
                    readonly property var actions: root.panelActions(index)
                    readonly property int panelIndex: index
                    x: root.panelX(index)
                    y: root.panelY(index)
                    width: root.panelWidth
                    height: Math.max(root.rowHeight, actions.length * root.rowHeight)
                    color: "#252525"
                    radius: 5
                    border.color: "#555555"
                    border.width: 1
                    z: 10 + index

                    Repeater {
                        model: panel.actions.length

                        delegate: Item {
                            readonly property var itemData: panel.actions[index]
                            x: 0
                            y: index * root.rowHeight
                            width: panel.width
                            height: root.rowHeight

                            Rectangle {
                                anchors.fill: parent
                                anchors.margins: 1
                                color: rowMouse.containsMouse && !itemData.is_separator ? "#3a3a3a" : "transparent"
                                radius: 3
                            }

                            Rectangle {
                                visible: itemData.is_separator
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.leftMargin: 8
                                anchors.rightMargin: 8
                                height: 1
                                color: "#666666"
                            }

                            Text {
                                visible: !itemData.is_separator
                                anchors.left: parent.left
                                anchors.right: arrow.left
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.leftMargin: 10
                                color: itemData.is_default ? "#ffffff" : "#eeeeee"
                                font.pixelSize: 13
                                text: itemData.label || ""
                                elide: Text.ElideRight
                            }

                            Text {
                                id: arrow
                                visible: itemData.submenu && itemData.submenu.length > 0
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.rightMargin: 8
                                color: "#dddddd"
                                font.pixelSize: 13
                                text: "›"
                            }

                            MouseArea {
                                id: rowMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                acceptedButtons: Qt.LeftButton | Qt.RightButton
                                onEntered: function() {
                                    if (!itemData.is_separator) root.openSubmenu(panel.panelIndex, index, itemData);
                                }
                                onClicked: function(mouse) {
                                    if (mouse.button === Qt.RightButton) {
                                        root.finish("");
                                    } else if (!itemData.is_separator && (!itemData.submenu || itemData.submenu.length === 0)) {
                                        root.finish(itemData.id || "");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
"##;

pub struct WarmPresenter {
    program: PathBuf,
    show_icons: bool,
    idle_timeout: Duration,
    child: Option<WarmUiChild>,
    last_used_at: Option<Instant>,
}

struct WarmUiChild {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl WarmPresenter {
    pub fn from_config(config: &LoadedConfig, idle_timeout: Duration) -> Result<Self, String> {
        match config.app.ui.as_ref().and_then(|ui| ui.frontend.as_deref()) {
            None | Some("qt") => Ok(Self {
                program: find_ui_program("kanyrun-ui"),
                show_icons: config
                    .app
                    .ui
                    .as_ref()
                    .and_then(|ui| ui.show_icons)
                    .unwrap_or(true),
                idle_timeout,
                child: None,
                last_used_at: None,
            }),
            Some(other) => Err(format!("unsupported ui frontend: {other}")),
        }
    }

    pub fn stop_if_idle(&mut self) {
        let Some(last_used_at) = self.last_used_at else {
            return;
        };
        if last_used_at.elapsed() >= self.idle_timeout {
            self.shutdown();
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.stdin.flush();
            let _ = child.child.kill();
            let _ = child.child.wait();
        }
        self.last_used_at = None;
    }

    fn ensure_child(&mut self) -> Result<(), String> {
        if self.child.is_some() {
            return Ok(());
        }

        let display = self.program.display().to_string();
        let mut child = Command::new(&self.program)
            .arg("--serve-stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("failed to spawn warm ui {display}: {error}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("failed to open stdin for warm ui {display}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("failed to open stdout for warm ui {display}"))?;

        self.child = Some(WarmUiChild {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        });
        Ok(())
    }

    fn request_via_child(&mut self, input: &str) -> Result<Option<String>, String> {
        self.ensure_child()?;
        let response = self.try_request_via_child(input);
        if response.is_ok() {
            self.last_used_at = Some(Instant::now());
            return response;
        }

        self.shutdown();
        self.ensure_child()?;
        let retry = self.try_request_via_child(input);
        if retry.is_ok() {
            self.last_used_at = Some(Instant::now());
        }
        retry
    }

    fn try_request_via_child(&mut self, input: &str) -> Result<Option<String>, String> {
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| "warm ui child is not running".to_string())?;

        writeln!(child.stdin, "{}", input.len())
            .and_then(|_| child.stdin.write_all(input.as_bytes()))
            .and_then(|_| child.stdin.flush())
            .map_err(|error| format!("failed to send payload to warm ui: {error}"))?;

        let mut response = String::new();
        let bytes = child
            .stdout
            .read_line(&mut response)
            .map_err(|error| format!("failed to read response from warm ui: {error}"))?;
        if bytes == 0 {
            return Err("warm ui closed its output stream unexpectedly".into());
        }

        let trimmed = response.trim_end_matches(['\r', '\n']).trim().to_string();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed))
        }
    }
}

impl Drop for WarmPresenter {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl MenuPresenter for WarmPresenter {
    fn show_menu(&mut self, menu: &MenuModel) -> Result<Option<String>, String> {
        let input = serialize_menu(menu, self.show_icons)?;
        self.request_via_child(&input)
    }
}

trait UiProcessRunner {
    fn run(&self, program: &Path, input: &str) -> Result<String, String>;
}

struct SystemUiProcessRunner;

impl UiProcessRunner for SystemUiProcessRunner {
    fn run(&self, program: &Path, input: &str) -> Result<String, String> {
        let display = program.display().to_string();
        let mut child = Command::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to spawn {display}: {error}"))?;

        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| format!("failed to open stdin for {display}"))?;
            stdin
                .write_all(input.as_bytes())
                .map_err(|error| format!("failed to write menu payload to {display}: {error}"))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|error| format!("failed to wait for {display}: {error}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return if stderr.is_empty() {
                Err(format!("ui command {display} exited with status {}", output.status))
            } else {
                Err(format!(
                    "ui command {display} exited with status {}: {stderr}",
                    output.status
                ))
            };
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

fn show_menu_with<R>(menu: &MenuModel, show_icons: bool, runner: &R, program: &Path) -> Result<Option<String>, String>
where
    R: UiProcessRunner,
{
    let input = serialize_menu(menu, show_icons)?;
    let output = runner.run(program, &input)?;

    Ok(output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string))
}

fn find_ui_program(name: &str) -> PathBuf {
    if let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|current| sibling_ui_program_for(&current, name))
    {
        return path;
    }

    if let Some(path) = find_in_path(name) {
        return path;
    }

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let local_build = repo_root.join(".build").join(name).join(name);
    if local_build.is_file() {
        return local_build;
    }

    PathBuf::from(name)
}

fn sibling_ui_program_for(current: &Path, name: &str) -> Option<PathBuf> {
    let candidate = current.parent()?.join(name);
    candidate.is_file().then_some(candidate)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;

    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn serialize_menu(menu: &MenuModel, show_icons: bool) -> Result<String, String> {
    serde_json::to_string(&MenuPayload::from_menu(menu, show_icons))
        .map_err(|error| format!("failed to serialize menu payload: {error}"))
}

#[derive(Serialize)]
struct MenuPayload {
    title: String,
    show_icons: bool,
    actions: Vec<MenuActionPayload>,
}

impl MenuPayload {
    fn from_menu(menu: &MenuModel, show_icons: bool) -> Self {
        Self {
            title: menu.title.clone(),
            show_icons,
            actions: menu.actions.iter().map(MenuActionPayload::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct MenuActionPayload {
    id: String,
    label: String,
    icon: Option<String>,
    is_default: bool,
    is_separator: bool,
    submenu: Option<Vec<MenuActionPayload>>,
}

impl From<&crate::menu::MenuAction> for MenuActionPayload {
    fn from(action: &crate::menu::MenuAction) -> Self {
        Self {
            id: action.id.clone(),
            label: action.label.clone(),
            icon: action.icon.clone(),
            is_default: action.is_default,
            is_separator: action.is_separator,
            submenu: action
                .submenu
                .as_ref()
                .map(|submenu| submenu.iter().map(MenuActionPayload::from).collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{show_menu_with, sibling_ui_program_for};
    use crate::menu::{ActionCommand, MenuAction, MenuModel};
    use serde_json::Value;
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    struct FakeUiRunner {
        output: Result<String, String>,
        program: RefCell<Option<String>>,
        input: RefCell<Option<String>>,
    }

    impl FakeUiRunner {
        fn success(output: &str) -> Self {
            Self {
                output: Ok(output.into()),
                program: RefCell::new(None),
                input: RefCell::new(None),
            }
        }
    }

    impl super::UiProcessRunner for FakeUiRunner {
        fn run(&self, program: &Path, input: &str) -> Result<String, String> {
            self.program
                .replace(Some(program.display().to_string()));
            self.input.replace(Some(input.to_string()));
            self.output.clone()
        }
    }

    #[test]
    fn passes_structured_menu_payload_and_returns_selected_action() {
        let runner = FakeUiRunner::success("copy\n");
        let menu = sample_menu();

        let selected = show_menu_with(&menu, true, &runner, Path::new("fake-ui")).unwrap();

        assert_eq!(selected.as_deref(), Some("copy"));
        assert_eq!(runner.program.borrow().as_deref(), Some("fake-ui"));

        let payload: Value = serde_json::from_str(runner.input.borrow().as_deref().unwrap()).unwrap();
        assert_eq!(payload["title"], "Text");
        assert_eq!(payload["show_icons"], true);
        assert_eq!(payload["actions"][0]["id"], "search");
        assert_eq!(payload["actions"][0]["label"], "Search");
        assert_eq!(payload["actions"][0]["is_default"], true);
        assert_eq!(payload["actions"][1]["id"], "copy");
    }

    #[test]
    fn serializes_submenu_items_in_payload() {
        let runner = FakeUiRunner::success("search-github\n");
        let menu = submenu_menu();

        let selected = show_menu_with(&menu, true, &runner, Path::new("fake-ui")).unwrap();

        assert_eq!(selected.as_deref(), Some("search-github"));

        let payload: Value = serde_json::from_str(runner.input.borrow().as_deref().unwrap()).unwrap();
        assert_eq!(payload["actions"][0]["submenu"][0]["id"], "search-default");
        assert_eq!(payload["actions"][0]["submenu"][1]["id"], "search-github");
    }

    #[test]
    fn serializes_separator_items_in_payload() {
        let runner = FakeUiRunner::success("copy\n");
        let menu = menu_with_separator();

        let selected = show_menu_with(&menu, true, &runner, Path::new("fake-ui")).unwrap();

        assert_eq!(selected.as_deref(), Some("copy"));

        let payload: Value = serde_json::from_str(runner.input.borrow().as_deref().unwrap()).unwrap();
        assert_eq!(payload["actions"][1]["is_separator"], true);
        assert_eq!(payload["actions"][1]["label"], "---");
    }

    #[test]
    fn returns_none_when_ui_dismisses_without_selection() {
        let runner = FakeUiRunner::success("\n");
        let menu = sample_menu();

        let selected = show_menu_with(&menu, true, &runner, Path::new("fake-ui")).unwrap();

        assert_eq!(selected, None);
    }

    #[test]
    fn prefers_sibling_ui_binary_when_present() {
        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let app_path = bin_dir.join("kanyrun");
        let ui_path = bin_dir.join("kanyrun-ui");
        fs::write(&app_path, "").unwrap();
        fs::write(&ui_path, "").unwrap();

        assert_eq!(
            sibling_ui_program_for(&app_path, "kanyrun-ui"),
            Some(ui_path)
        );
    }

    #[test]
    fn falls_back_to_program_name_when_sibling_ui_binary_is_absent() {
        let temp = TempDir::new().unwrap();
        let app_path = temp.path().join("bin").join("kanyrun");

        assert_eq!(sibling_ui_program_for(&app_path, "kanyrun-ui"), None);
    }

    fn sample_menu() -> MenuModel {
        MenuModel {
            id: "text".into(),
            title: "Text".into(),
            actions: vec![
                MenuAction {
                    id: "search".into(),
                    label: "Search".into(),
                    icon: Some("system-search".into()),
                    is_default: true,
                    command: Some(ActionCommand::CopyText),
                    submenu: None,
                    is_separator: false,
                },
                MenuAction {
                    id: "copy".into(),
                    label: "Copy".into(),
                    icon: None,
                    is_default: false,
                    command: Some(ActionCommand::CopyText),
                    submenu: None,
                    is_separator: false,
                },
            ],
        }
    }

    fn menu_with_separator() -> MenuModel {
        MenuModel {
            id: "text".into(),
            title: "Text".into(),
            actions: vec![
                MenuAction {
                    id: "search".into(),
                    label: "Search".into(),
                    icon: Some("system-search".into()),
                    is_default: true,
                    command: Some(ActionCommand::CopyText),
                    submenu: None,
                    is_separator: false,
                },
                MenuAction {
                    id: "separator-1".into(),
                    label: "---".into(),
                    icon: None,
                    is_default: false,
                    command: Some(ActionCommand::Separator),
                    submenu: None,
                    is_separator: true,
                },
                MenuAction {
                    id: "copy".into(),
                    label: "Copy".into(),
                    icon: None,
                    is_default: false,
                    command: Some(ActionCommand::CopyText),
                    submenu: None,
                    is_separator: false,
                },
            ],
        }
    }

    fn submenu_menu() -> MenuModel {
        MenuModel {
            id: "text".into(),
            title: "Text".into(),
            actions: vec![MenuAction {
                id: "search".into(),
                label: "Search".into(),
                icon: Some("system-search".into()),
                is_default: true,
                command: None,
                submenu: Some(vec![
                    MenuAction {
                        id: "search-default".into(),
                        label: "Search: hello world".into(),
                        icon: None,
                        is_default: true,
                        command: Some(ActionCommand::CopyText),
                        submenu: None,
                        is_separator: false,
                    },
                    MenuAction {
                        id: "search-github".into(),
                        label: "GitHub search".into(),
                        icon: None,
                        is_default: false,
                        command: Some(ActionCommand::CopyText),
                        submenu: None,
                        is_separator: false,
                    },
                ]),
                is_separator: false,
            }],
        }
    }
}
