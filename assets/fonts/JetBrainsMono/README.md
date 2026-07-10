# JetBrainsMono Nerd Font (bundled)

ComandOS incluye la variante **Mono** de JetBrains Mono con parche Nerd Fonts
para garantizar que la app se vea igual en cualquier máquina (WSL, Linux, macOS)
sin depender de `apt`/`brew`/`fc-cache` externos.

## Archivos

| Archivo                                       | Peso   | Uso                            |
| --------------------------------------------- | ------ | ------------------------------ |
| `JetBrainsMonoNerdFontMono-Regular.ttf`       | 2.5 MB | Terminal + UI (peso normal)    |
| `JetBrainsMonoNerdFontMono-Italic.ttf`        | 2.5 MB | Cursivas                       |
| `JetBrainsMonoNerdFontMono-Bold.ttf`          | 2.5 MB | Negritas                       |
| `JetBrainsMonoNerdFontMono-BoldItalic.ttf`    | 2.5 MB | Negrita + cursiva              |

Se elige la variante **Mono** (ancho estricto) — no *Propo* — para que el
terminal alinee columnas correctamente.

## Instalación

`install.sh` copia estos TTF a `~/.local/share/fonts/comandos/` (Linux/WSL) o
`~/Library/Fonts/` (macOS) y ejecuta `fc-cache -f` si aplica. Idempotente.

## Licencia

- **JetBrains Mono**: SIL Open Font License 1.1 — ver `OFL.txt`
- **Parche Nerd Fonts**: MIT — <https://github.com/ryanoasis/nerd-fonts>

Redistribución permitida siempre que se conserven los archivos de licencia.

## Actualizar

Descargar el release más reciente y reemplazar los 4 TTF:

```bash
curl -sL -o /tmp/jbnf.zip \
  https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.zip
python3 -c "
import zipfile
z = zipfile.ZipFile('/tmp/jbnf.zip')
for n in ['JetBrainsMonoNerdFontMono-Regular.ttf',
          'JetBrainsMonoNerdFontMono-Italic.ttf',
          'JetBrainsMonoNerdFontMono-Bold.ttf',
          'JetBrainsMonoNerdFontMono-BoldItalic.ttf']:
    z.extract(n, 'assets/fonts/JetBrainsMono/')
"
```
