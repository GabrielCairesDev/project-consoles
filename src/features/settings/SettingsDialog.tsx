import { useId } from "react";
import { RotateCw, TriangleAlert } from "lucide-react";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { Switch } from "../../components/Switch";
import type { Console } from "../../domain/console";
import type { ConsoleSettingsControl } from "./useConsoleSettings";
import styles from "./SettingsDialog.module.css";

interface SettingsDialogProps {
  console: Console;
  /** Estado compartilhado com a página (a grade reage às mudanças). */
  control: ConsoleSettingsControl;
  onClose: () => void;
}

/** Opções de jogo de um console. */
export function SettingsDialog({ console, control, onClose }: SettingsDialogProps) {
  const { state, saving, saveError, update, retry } = control;
  const gameSectionId = useId();

  return (
    <Dialog title={`Configurações · ${console.name}`} onClose={onClose}>
      {state.status === "loading" && (
        <div className={styles.skeleton} role="status" aria-label="Carregando configurações" />
      )}

      {state.status === "error" && (
        <div className={styles.failure} role="alert">
          <TriangleAlert className={styles.failureIcon} aria-hidden="true" />
          <p className={styles.failureText}>{state.message}</p>
          <Button variant="ghost" icon={RotateCw} onClick={retry}>
            Tentar novamente
          </Button>
        </div>
      )}

      {state.status === "ready" && (
        <section className={styles.section} aria-labelledby={gameSectionId}>
          <h3 id={gameSectionId} className={styles.sectionTitle}>
            Jogo
          </h3>
          <Switch
            label="Continuar de onde parou"
            description="Ao fechar o jogo, o progresso é salvo; ao abrir de novo, ele continua do mesmo ponto. Desligado, o jogo sempre começa do início, sem apagar o que já foi salvo."
            checked={state.settings.resume}
            disabled={saving}
            onChange={(resume) => update({ resume })}
          />
        </section>
      )}

      {saveError && (
        <p className={styles.error} role="alert">
          {saveError}
        </p>
      )}
    </Dialog>
  );
}
