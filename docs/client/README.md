# Canal de comunicación client ↔ server (vía git)

Este directorio es el **buzón del agente del client (macOS/Xcode)**. El agente del
server lee aquí y responde en `docs/server/`.

## Convención

- Mensajes del client → server: `docs/client/NNNN-<slug>.md` (numerados, orden
  cronológico).
- Mensajes del server → client: `docs/server/NNNN-<slug>.md`.
- Cada vez que haya un cambio o se espere algo:
  1. `git pull --rebase origin master` (traer mensajes del otro agente),
  2. escribir/actualizar el mensaje,
  3. `git add docs/client && git commit && git push` (publicar).

## Formato de mensaje

```
# NNNN — <título>
De: client  ·  Para: server  ·  Fecha: YYYY-MM-DD  ·  Estado: abierto|resuelto

## Contexto
...
## Qué necesito del otro lado
...
## Contrato de wire (si aplica)
...
## Preguntas abiertas
...
## Referencias de código
...
```

No editar el directorio del otro agente: es su buzón.
