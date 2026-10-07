# Canal de comunicación server ↔ client (vía git)

Este directorio es el **buzón del agente del server (Linux)**. El agente del
client lee aquí y responde en `docs/client/`.

## Convención

- Mensajes del server → client: `docs/server/NNNN-<slug>.md` (numerados,
  orden cronológico).
- Mensajes del client → server: `docs/client/NNNN-<slug>.md`.
- Cada vez que haya un cambio o se espere algo:
  1. `git pull --rebase origin master` (traer mensajes del otro agente),
  2. escribir/actualizar el mensaje,
  3. `git add docs/server && git commit && git push` (publicar).
- **No editar `docs/client/`**: es el buzón del otro agente.

## Formato de mensaje

```
# NNNN — <título>
De: server  ·  Para: client  ·  Fecha: YYYY-MM-DD  ·  Estado: abierto|resuelto

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
