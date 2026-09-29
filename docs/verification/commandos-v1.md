# CommandOS 1.0 — registro de aceptación

Base: main `c0c68cf`. Rama: `implementation/comandos-v1`. Entorno de pruebas: `/tmp/comandos-v1-qa` (Python 3.10, pytest 9.1.1; dependencias en `/tmp/comandos-v1-qa-requirements.txt`).

## Decisiones de Jesús (2026-09-29, respuestas literales a propuestas concretas)

| ID | Respuesta | Alcance aplicado |
| --- | --- | --- |
| D1 | "Se conservan" | Congelado y Esperando respuesta solo cambian por acción humana. Solo Resuelto se reabre con un prompt aceptado y confirmado. Tras terminar un turno el pane queda sin marca (icono neutral). |
| D2 | "La del mockup" | Política `v1`: 10 XP por minuto de foco activo, nivel cada 1.000 XP, objetivo diario 100 min. Cancelados cuentan solo su tiempo activo; descansos no dan XP. Sin puntos retroactivos sobre historial previo. |
| D3 | "Global, ciclos manuales" | Un estilo de arte para toda la app (Alquimia inicial). Al terminar un foco no arranca el descanso automáticamente. |
| D4 | "09:00 · 15:00 · 21:00" | America/Mexico_City. Máx. 25 fuentes y tope ≈ US$0,25 por edición con un modelo económico. Una edición perdida por caída se marca como no publicada, sin recuperarla después. |
| D5 | "Sonido solo si te necesita" | Sonido en permiso/entrada pedida, error y fin de Pomodoro; turno terminado solo visual. Float 6 s; ráfagas del mismo proyecto se agrupan 10 s. Durante un foco solo suenan permisos. Push al Android si ningún cliente está visible desde hace 2 min. |
| D6 | "Proyecto + título" | Pantalla bloqueada: proyecto y título breve, sin extracto. Si el último cliente no puede sonar, suena el siguiente cliente visible. |

## Evidencia por bloque

(se completa al cerrar cada bloque)
