"""Genera config/cli-commands.json desde scraped/<cli>.json: lista plana por CLI,
con el nombre y la descripción exactos que muestra el menú `/` de cada CLI.
Solo añade lo que el menú no dice: chips de argumentos ya verificados (ARGS)."""
import json, re
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parents[1] / "config" / "cli-commands.json"
ARGS = {
    ("claude", "model"): {"argsFrom": "models", "args": ["claude-fable-5-1", "claude-opus-5-5", "claude-sonnet-5-5"]},
    ("claude", "effort"): {"args": ["low", "medium", "high", "max"]},
    ("claude", "add-dir"): {},
    ("codex", "cd"): {},
    ("grok", "model"): {"argsFrom": "models", "args": ["grok-4.6", "grok-4.5"]},
    ("grok", "effort"): {"args": ["low", "high"]},
    ("agy", "effort"): {"args": ["low", "high"]},
    ("agy", "add-dir"): {},
}

# El barrido por prefijos también captura alias: `/m` es `/model` y `/t` es `/theme`
# en Grok (ambos en el binario). None = alias de un comando que ya está en la lista.
ALIASES = {("grok", "m"): "model", ("grok", "t"): None}


def clean(desc):
    """Quita el estado de la sesión que se capturó: «(currently Opus 5.5 …)»."""
    return re.sub(r"\s*\(currently .*\)\s*$", "", desc)


def build(old):
    by_id = {c["id"]: c for c in old["clis"]}
    clis = []
    for cli_id in [c["id"] for c in old["clis"]]:
        src = json.loads((HERE / "scraped" / f"{cli_id}.json").read_text(encoding="utf-8"))
        cmds = []
        for name, desc in src["commands"]:
            name = ALIASES.get((cli_id, name), name)
            if name is None:
                continue
            desc = clean(desc)
            extra = ARGS.get((cli_id, name))
            cmd = {"text": f"/{name} " if extra is not None else f"/{name}", "description": desc}
            cmd.update(extra or {})
            cmds.append(cmd)
        cmds.sort(key=lambda c: c["text"])
        cli = {k: v for k, v in by_id[cli_id].items() if k != "groups"}
        cli.update({"pinnedVersion": src["version"], "verified": True,
                    "groups": [{"title": "", "icon": "", "commands": cmds}]})
        clis.append(cli)
    return {"version": 1, "source": "tools/cli-commands/scraped (menú / de cada CLI)", "clis": clis}


if __name__ == "__main__":
    old = json.loads(OUT.read_text(encoding="utf-8"))
    OUT.write_text(json.dumps(build(old), ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(OUT)
