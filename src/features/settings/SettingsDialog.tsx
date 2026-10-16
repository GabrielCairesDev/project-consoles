import { useId } from "react";
import { CircleAlert, CircleCheck, FolderOpen, RotateCw, TriangleAlert } from "lucide-react";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { Switch } from "../../components/Switch";
import type { Console } from "../../domain/console";
import { type BiosControl, useBios } from "./useBios";
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
  const bios = useBios(console.id, console.needsBios);

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
          {console.resumable ? (
            <Switch
              label="Continuar de onde parou"
              description="Ao fechar o jogo, o progresso é salvo; ao abrir de novo, ele continua do mesmo ponto. Desligado, o jogo sempre começa do início, sem apagar o que já foi salvo."
              checked={state.settings.resume}
              disabled={saving}
              onChange={(resume) => update({ resume })}
            />
          ) : (
            <Switch
              label="Continuar de onde parou"
              description={`Ainda não disponível no ${console.name}: o jogo sempre começa do início. Os saves feitos no próprio jogo continuam guardados.`}
              checked={false}
              disabled
              onChange={() => {}}
            />
          )}
        </section>
      )}

      {console.needsBios && <BiosSection consoleName={console.name} control={bios} />}

      {saveError && (
        <p className={styles.error} role="alert">
          {saveError}
        </p>
      )}
    </Dialog>
  );
}

/** BIOS do console: situação e a escolha do arquivo. */
function BiosSection({ consoleName, control }: { consoleName: string; control: BiosControl }) {
  const { state, choosing, chooseError, choose, retry } = control;
  const sectionId = useId();
  const installed = state.status === "ready" && state.bios.installed;

  return (
    <section className={styles.section} aria-labelledby={sectionId}>
      <h3 id={sectionId} className={styles.sectionTitle}>
        BIOS
      </h3>

      {state.status === "loading" && (
        <div className={styles.skeleton} role="status" aria-label="Carregando BIOS" />
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
        <div className={styles.bios}>
          <div className={styles.biosText}>
            <p className={styles.biosStatus} data-installed={installed || undefined}>
              {installed ? (
                <CircleCheck className={styles.biosIcon} aria-hidden="true" />
              ) : (
                <CircleAlert className={styles.biosIcon} aria-hidden="true" />
              )}
              {installed ? "BIOS instalado" : "Nenhum BIOS escolhido"}
            </p>
            <p className={styles.biosHelp}>
              {`O ${consoleName} só liga com o BIOS do seu console (por exemplo, SCPH1001.BIN, de 512 KB). O app guarda uma cópia.`}
            </p>
          </div>
          <Button variant="ghost" icon={FolderOpen} onClick={choose} disabled={choosing}>
            {installed ? "Trocar BIOS…" : "Escolher BIOS…"}
          </Button>
        </div>
      )}

      {chooseError && (
        <p className={styles.error} role="alert">
          {chooseError}
        </p>
      )}
    </section>
  );
}
