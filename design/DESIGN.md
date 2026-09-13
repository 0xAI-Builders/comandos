# Controles de ComandOS

La interfaz usa tarjetas para comparar paneles, un selector para preparar cambios y un compositor táctil para escribir en remoto. Los controles comparten la configuración observada y las rutas del servidor.

Los colores proceden del tema de `dash/index.html`. `workspace.css` usa sus variables: fondo, paneles, bordes, texto, texto secundario y acento. El tema oscuro usa texto `#EAF0FB` sobre panel `#121722`; el botón principal combina acento `#8B7CFF` y texto `#0A0D13`.

Se conservan las familias sans y mono del producto. Las acciones principales tienen al menos 44 px de altura. El selector utiliza dos columnas en escritorio y una en teléfonos. Los diálogos permiten desplazamiento interno y adaptan su altura al viewport.

Solo **Aplicar cambios** ejecuta el borrador de configuración. Los controles de cuenta, CLI, modelo y esfuerzo no envían operaciones al seleccionarse. Las opciones incompatibles explican el motivo. La recuperación aparece en el mismo selector cuando hay una operación pendiente de restauración.

El chat oculto conserva el estado y libera espacio. El resumen de sesiones renderiza tarjetas, no terminales. La selección remota utiliza texto del panel en una vista estable. La entrada táctil conserva composición, autocorrección y borradores antes del envío explícito.

Las métricas distinguen configuración de uso observado. No se muestran estimaciones de ahorro como resultados medidos. Las limitaciones de cada CLI forman parte del editor de perfiles para evitar controles que aparenten cambios en caliente.

Las [referencias](references.md) y la [prueba visual](proof.html) registran el alcance de la revisión.
