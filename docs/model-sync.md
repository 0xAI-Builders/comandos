# Sincronización de modelos

El selector combina el registro mantenido por ComandOS con los catálogos locales de Codex y Grok. Solo incorpora modelos que el CLI marca como visibles y cuyas capacidades de esfuerzo están declaradas. Las rutas, cuentas, autenticación y banderas de verificación no cambian al actualizar el catálogo. Tampoco se modifica el modelo de una sesión abierta.

La caché del registro se invalida cuando cambia el archivo de proveedores o el catálogo de cualquiera de esos CLIs. Se copian identificador, esfuerzos, valor predeterminado y ventana de contexto declarados; los nombres y metadatos manuales se conservan. Una aparición en el catálogo es evidencia local de disponibilidad, no una prueba de inferencia ni una garantía para todas las cuentas.

El watcher vuelve a descubrir modelos cuando cambia un catálogo aunque no se haya actualizado el ejecutable. Distingue variantes de la misma generación y sufijos compuestos, como Sol/Luna y Build Fast. Las cadenas halladas únicamente en binarios siguen siendo detecciones sin disponibilidad comprobada; no se incorporan al selector por ese motivo.

Los lectores limitan el tamaño del caché y conservan el registro base ante datos ilegibles, malformados o excesivamente anidados. No descargan modelos ni cambian las credenciales. La frescura depende de que el CLI haya actualizado su propio catálogo local.
