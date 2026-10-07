# Comprobaciones y limpieza del 7 de octubre de 2026

Código verificado: `b95b4bfd18c1729160cf08572e7fc561300f1cb3`. Se corrigieron la prueba nativa de namespaces y el aislamiento de suscripciones y logging por cliente MCP. Las compilaciones Linux y macOS y el instalador completo privado pasan. La migración conserva pendientes de aceptación; no se hizo el cambio de instalación durante este bloque.

La colección completa ejecutó 424 suites: 3,319 casos aprobados, un fallo de puerto ocupado en una prueba HTTP y 68 casos ignorados. Se corrigió la selección de puerto conservando el listener exclusivo y las seis pruebas HTTP pasan. La evidencia combinada cubre 3,320 casos ordinarios. El comando completo original mantiene su salida 101; no se presenta como una ejecución con salida cero. Las pruebas ignoradas incluyen ayudantes que otras pruebas invocan como subprocessos privados, además de comprobaciones explícitas de recursos e instalación.

Clippy pasó con advertencias tratadas como errores para los cinco paquetes afectados y todos sus targets. El instalador privado comprobó los 63 archivos web, los binarios nativos reales, los alias y 33 reemplazos de frontends. Las lecturas privadas con 172 MB tardaron 4,586 y 3,438 µs, dentro de su límite de 50 ms.

Los requisitos de rendimiento siguen abiertos: escritura Mirror release de 3,823 µs frente a menos de 3,000 µs; 200 hooks completos sin errores, mediana de 57.479 ms y p95 de 108.935 ms frente a 100 ms. Todos los trabajadores privados terminaron y la fuente privada conservó identidad y bytes. La medición inicial sin optimizar y estos resultados fallidos se conservan en los recibos. No se relajaron los presupuestos.

Se eliminaron 73 directorios de compilación o pruebas, con 1,265,279,008,768 bytes asignados. Los directorios vinculados a procesos activos y la caché release de 2.2 GiB necesaria para el trabajo pendiente se conservaron. Los perfiles de desarrollo y pruebas omiten símbolos e incremental por defecto para limitar acumulación; las variables habituales de Cargo permiten activar símbolos cuando se requieren. No se borraron fuentes, datos, releases instaladas ni historiales de Codex o Claude.

La aceptación GUI completa, la reducción de memoria aplazada por Jesús y el cambio de instalación siguen separados de estas comprobaciones. El smoke GTK ordinario comprobó enlace con GTK sin abrir un display. Las 459 entradas del inventario GUI no se presentan como 459 comprobaciones individuales. No se aplicó la reparación pendiente de 796 registros ni se promovieron o sellaron dominios del almacenamiento vivo.

Recibo consolidado: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-integration-acp/docs/verification/native-functional-close-20261007.json

Los binarios Linux conservados están en /home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/functional-close-20261007/native-install-artifacts-b95b4bfd.

En la Mac nueva, los binarios macOS conservados están en /Users/jesuscreativestudio/.local/share/comandos-validation/darwin-runtime-20261006/build-b95b4bfd-native-release/artifacts.
