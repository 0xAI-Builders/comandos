# Cambio de autoridad del estado

S6 implementa cambio de modo, exportación, sellado, recuperación desde respaldo y guardia de vuelta de release. La validación de código usa HOME, SQLite y censo de procesos privados. No se ha migrado ni activado el estado real con este cambio.

## Ensayo previo

El controlador debe ejecutar el ensayo sobre una copia del HOME real antes de activar dominios. El comando solo lee el origen; el migrador, las escrituras y los enlaces de release operan dentro de un HOME nuevo en el directorio temporal.

```sh
cargo xtask state-drill --source-home /home/someguy --binary /home/someguy/codebase/0xJesus/ComandOS/.build/target-integration-acp/release/comandos
```

El ensayo ejecuta dry-run, migración, escrituras sintéticas en mirror, verificaciones, cambio a unified, nuevas escrituras, traslado SQLite, rollback de release a protocolo 0, comparación de lectores legado, demote y recuperación destructiva desde respaldo. Emplea fechas sintéticas exclusivamente dentro de esa copia; no acredita las 24 horas ni los siete días del entorno vivo. `--keep` conserva la copia y el informe imprime su ruta absoluta. Las bases de SQLite ausentes en el origen se indican mediante la lista de traslados realizados.

La copia coherente del origen real contiene 796 filas de `event_receipts` sin su evento padre. El preflight del traslado SQLite rechaza esa base antes de escribir su respaldo o sus tablas de destino. El informe está en `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/real-data-foreign-key-checks-7310ec8b.json`. El comando anterior seguirá rechazándolo hasta resolver su integridad.

El ensayo completo pasó sobre una copia privada reparada. Esa copia archiva los recibos con sus rowids, tipos y bytes exactos, conserva las demás tablas y queda sin referencias huérfanas. La reparación no se aplicó al HOME real. Jesús debe elegir entre conservar la limpieza anterior archivando los recibos o restaurar sus alertas antiguas desde el respaldo. El resultado de la reparación privada está en `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/orphan-receipts-private-repair-preview.json`.

## Evidencia de integración

Los ensayos de estado corresponden al código compilado en `1e647ebdec6012340637cf530ff84f116f7943a9`. El release incorpora la corrección de ayuda de `1a5e6f58ed58e55ca172f7607bc85991c261d1f8`; los módulos de estado, app y web mantienen su código. El build usa Linux x86_64 con el SDK GTK privado. Su procedencia y hashes están en `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/native-final-source-release-1a5e6f58-provenance.json`.

| Comprobación | Resultado y alcance | Evidencia |
| --- | --- | --- |
| Almacenamiento | 46 pruebas pasan, incluida la negativa ante recibos huérfanos. | `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/orphan-receipts-preflight-green.log` |
| Extensiones nativas | Pasan 3 pruebas de runtime y 5 HTTP. Una fixture de actor permanece ignorada y no cuenta como prueba pasada. | `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/operations-optional-extensions-runtime-green.log`; `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/operations-optional-extensions-http-installed-green.log` |
| Ayuda del CLI | Pasan las 7 pruebas de arranque y Clippy estricto. El binario release responde a `dash --help` y `dash -h` sin HOME, token, directorio runtime o listener. | `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/dash-help-no-effects-final-green.log`; `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/dash-help-no-effects-clippy.log`; `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/dash-help-release-1a5e6f58-proof.json` |
| Estimación de traslado | Copia privada en disco de 218.177.536 bytes: 3.314 ms, presupuesto de 4.000 ms. No mide una pausa del entorno vivo. | `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/real-usage-copy-estimate-1e647ebd.json` |
| Ensayo completo | 10.686 fuentes, 13 dominios y las cinco bases SQLite; vuelta de release, lectores legado, demote y restauración exacta desde respaldo pasan. | `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/state-drill-real-data-1e647ebd-repaired-tmpfs.log`; `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/state-drill-real-data-1e647ebd-repaired-tmpfs-provenance.json` |
| Instalación privada | CLI y app se preparan sin activar enlaces ni servicios vivos. Los 61 archivos del manifiesto web coinciden byte a byte. | `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/native-final-private-stage-1a5e6f58-provenance.json` |
| Instalación en HOME real | Pasa exclusivamente el ensayo en seco. Las seis rutas activas comprobadas conservan bytes e identidad; el release permanece sin activar. | `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/native-final-real-home-install-preview-1e647ebd.json` |
| Admisión del tablero | El release anterior de la misma rama responde HTTP 200 con los 48 módulos seleccionados en un HOME privado. La configuración ausente produce 503 sin fallback. | `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/native-final-admission-private-selection-proof.json` |

El ensayo completo usa la copia reparada y almacenamiento tmpfs. Acredita el recorrido funcional y la vuelta atrás sobre esa copia. Las fechas sintéticas no acreditan las ventanas vivas; tmpfs tampoco acredita durabilidad física o rendimiento de disco.

La medición final de GTK falla los presupuestos de memoria. El escenario usa veinte terminales nativas, el tablero WASM listo y 120 segundos en una VM Linux privada de la Mac nueva. Su renderizado es por software. Los bytes de la app medida coinciden con el release indicado arriba.

| Proceso | RSS final, MB decimales | Límite, MB | Resultado |
| --- | ---: | ---: | --- |
| App | 163,721 | 130 | Falla |
| WebKit | 355,946 | 146 | Falla |
| Network | 55,284 | 885 | Pasa |

La procedencia, los valores exactos y la limpieza de los recursos propios están en `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/mac-nueva-browser/gtk-native-final-v53-gates.json`. La evidencia no acredita plataformas reales ni la prueba de una hora. Los fallos de memoria siguen abiertos.

El cierre exige resolver la integridad del origen real, aprobar recursos y plataformas, ejecutar la secuencia viva y completar la retirada comprobada. Subir esta rama o preparar los artefactos privados no completa esas operaciones.

## Secuencia de activación

Estas operaciones quedan pendientes de ejecutar por el controlador. No requieren reiniciar tmux ni detener sesiones. Si un escritor todavía usa una release incompatible, el cambio se pospone hasta que pueda adoptar una release capaz sin interrumpirlo.

1. Integrar S5 de todos los consumidores del dominio, instalar la release capaz y retargetear los hooks. Mantener `legacy` mientras falte algún consumidor.
2. Inventariar y ejecutar `comandos state migrate --dry-run`. Revisar fuentes sin dominio, estimación SQLite y errores. Ejecutar después `comandos state migrate` para preparar mirror y respaldos con hash.
3. En mirror, medir mediana de 200 escrituras de hook, tamaño de base y RSS del frente. Ejecutar `comandos state verify --domain ui-docs` tres veces con al menos 24 horas entre la primera y la última. Registrar errores: una verificación fallida reinicia la ventana válida.
4. Ejecutar `comandos state flip ui-docs unified --repo /home/someguy/codebase/0xJesus/ComandOS`. Requiere mirror, tres verificaciones limpias de la ventana actual, una comparación limpia en ese momento y censo sin escritores incompatibles.
5. Realizar el simulacro vivo: crear un snippet desde el tablero, ejecutar `comandos state demote ui-docs`, comprobar el snippet desde el lector de archivo y volver a unified tras cumplir de nuevo la ventana de verificaciones. Documentar el resultado antes de activar los demás dominios.
6. Trasladar las bases en orden: `db-operator`, `db-news`, `db-operations`, `db-app-state`, `db-usage`, mediante `comandos state move DOMINIO`. Si el presupuesto de copia supera cuatro segundos, no forzar el traslado mientras haya agentes activos.
7. Tras siete días en unified sin incidencias y con el OK de Jesús, ejecutar `comandos state seal ui-docs --yes`. La confirmación explícita no evita la puerta temporal. Primero se verifica el respaldo; se confirma el guardia durable antes de retirar los archivos.

## Vuelta atrás

`comandos state demote ui-docs` devuelve unified a mirror tras comprobar que los archivos están al día. `comandos state demote --all` valida primero todos los dominios de archivo y también revierte las bases trasladadas. Un dominio sealed exige `comandos state export-legacy ui-docs`: reproduce sus bytes, vuelve a unified y permite después demote.

`comandos install --rollback-release` ejecuta esa guardia antes de cambiar el enlace hacia una release con protocolo menor que 2. Abortará ante un dominio sealed sin exportar o una comparación fallida; el enlace anterior sigue activo.

Como último recurso, `comandos state rollback RUN_ID --perder-desde-el-respaldo` restaura las fuentes del run y los primeros respaldos de los traslados SQLite posteriores. Requiere un censo sin escritores activos; no detiene procesos. Conserva una copia de las fuentes actuales y renombra la base en vez de borrarla. Su salida identifica el intervalo de datos perdido y la ruta absoluta de la base archivada. Sin la bandera explícita, rechaza la operación y propone demote.
