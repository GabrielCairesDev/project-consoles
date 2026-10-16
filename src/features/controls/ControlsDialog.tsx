import { RotateCcw, RotateCw, TriangleAlert } from "lucide-react";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import type { Console } from "../../domain/console";
import { sameBindings } from "../../domain/controls";
import { GameBoyController } from "./GameBoyController";
import { GenesisController } from "./GenesisController";
import type { ControllerProps } from "./KeyButton";
import { PlayStationController } from "./PlayStationController";
import { SmsController } from "./SmsController";
import { useControls } from "./useControls";
import styles from "./ControlsDialog.module.css";

/** Desenho do controle de cada console jogável. */
const CONTROLLERS: Record<
  string,
  (props: ControllerProps) => React.JSX.Element
> = {
  "master-system": SmsController,
  "mega-drive": GenesisController,
  "game-boy": GameBoyController,
  playstation: PlayStationController,
};

interface ControlsDialogProps {
  console: Console;
  onClose: () => void;
}

/** Configuração das teclas do controle de um console. */
export function ControlsDialog({ console, onClose }: ControlsDialogProps) {
  const controls = useControls(console.id);
  const { load, capturing, saving, notice, error } = controls;
  const info = load.status === "ready" ? load.info : null;
  const isDefault = info ? sameBindings(info.bindings, info.defaults) : true;
  const Controller = CONTROLLERS[console.id] ?? SmsController;

  return (
    <Dialog
      title={`Controles · ${console.name}`}
      onClose={onClose}
      footer={
        <Button
          variant="ghost"
          icon={RotateCcw}
          onClick={controls.resetToDefaults}
          disabled={!info || isDefault || saving}
        >
          Restaurar padrão
        </Button>
      }
    >
      {load.status === "loading" && (
        <div
          className={styles.skeleton}
          role="status"
          aria-label="Carregando controles"
        />
      )}

      {load.status === "error" && (
        <div className={styles.failure} role="alert">
          <TriangleAlert className={styles.failureIcon} aria-hidden="true" />
          <p className={styles.failureText}>{load.message}</p>
          <Button variant="ghost" icon={RotateCw} onClick={controls.retry}>
            Tentar novamente
          </Button>
        </div>
      )}

      {info && (
        <Controller
          bindings={info.bindings}
          capturing={capturing}
          disabled={saving}
          onSelect={controls.startCapture}
        />
      )}

      <p className={styles.notice} role="status" aria-live="polite">
        {notice}
      </p>
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
    </Dialog>
  );
}
