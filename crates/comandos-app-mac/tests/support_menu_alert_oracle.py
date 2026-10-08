"""Execute the original AST bodies with inert AppKit/I/O boundaries only."""
import ast
import json
import pathlib
import sys
import types

source = ast.parse(pathlib.Path(sys.argv[1]).read_text())
names = {"_build_menu", "_mi", "_prompt_rename", "_close_tab", "_tab_context_menu"}
methods = {}
for node in ast.walk(source):
    if isinstance(node, ast.FunctionDef) and node.name in names:
        node.decorator_list = []
        methods[node.name] = node
assert set(methods) == names


class Item:
    @classmethod
    def alloc(cls):
        return cls()

    def init(self):
        return self

    def initWithTitle_action_keyEquivalent_(self, title, action, key):
        self.title, self.action, self.key = title, action, key
        self.mask = 1
        return self

    def setTarget_(self, _):
        pass

    def setSubmenu_(self, menu):
        self.menu = menu

    def setKeyEquivalentModifierMask_(self, mask):
        self.mask = mask

    def setRepresentedObject_(self, value):
        self.represented = value


class Menu(Item):
    def __init__(self):
        self.title, self.items = "", []

    def initWithTitle_(self, title):
        self.title = title
        return self

    def addItem_(self, item):
        self.items.append(item)


class App:
    def setMainMenu_(self, menu):
        self.menu = menu


class Alert(Item):
    def __init__(self):
        self.buttons = []
        alerts.append(self)

    def setMessageText_(self, text):
        self.message = text

    def setInformativeText_(self, text):
        self.info = text

    def setAccessoryView_(self, field):
        self.field = field

    def addButtonWithTitle_(self, text):
        self.buttons.append(text)

    def runModal(self):
        return 1001  # Cancel prevents original downstream mutations.


class Field(Item):
    def initWithFrame_(self, rect):
        assert rect == (0, 0, 260, 24)
        return self

    def setStringValue_(self, text):
        self.text = text


sys.modules["AppKit"] = types.SimpleNamespace(
    NSEventModifierFlagCommand=1, NSEventModifierFlagShift=2
)
out = {}
for code, es in [("es", True), ("en", False)]:
    alerts = []
    app = App()
    env = {"ES": es, "NSMenu": Menu, "NSMenuItem": Item, "NSAlert": Alert,
           "NSTextField": Field, "NSApp": app, "NSMakeRect": lambda *v: v,
           "NSAlertFirstButtonReturn": 1000}
    cls = ast.ClassDef(name="Original", bases=[], keywords=[],
                       body=list(methods.values()), decorator_list=[])
    exec(compile(ast.fix_missing_locations(ast.Module(body=[cls], type_ignores=[])),
                 str(pathlib.Path(sys.argv[1]).resolve()), "exec"), env)
    owner = env["Original"]()
    owner._build_menu()
    context = [{"title": x.title, "action": x.action, "key": x.key,
                "represented": x.represented}
               for x in owner._tab_context_menu("tab").items]
    menu = [{"title": i.menu.title, "items": [
        {"title": x.title, "action": x.action, "key": x.key,
         "command": bool(x.mask & 1), "shift": bool(x.mask & 2)}
        for x in i.menu.items]} for i in app.menu.items]
    tab = {"is_hub": False, "key": "tab", "label": "雪 '$() «tab»"}
    owner._prompt_rename(tab)
    rename = alerts[-1]
    owner._close_tab(tab, confirm=True)
    close = alerts[-1]
    out[code] = {"menu": menu, "context": context, "rename": [rename.message, *rename.buttons],
                 "close": [close.message, close.info, *close.buttons]}
print(json.dumps(out, ensure_ascii=False))
