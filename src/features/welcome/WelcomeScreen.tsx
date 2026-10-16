import { Gamepad2 } from "lucide-react";
import styles from "./WelcomeScreen.module.css";

export function WelcomeScreen() {
  return (
    <main className={styles.screen}>
      <section className={styles.content} aria-labelledby="welcome-title">
        <div className={styles.mark} aria-hidden="true">
          <Gamepad2 className={styles.markIcon} strokeWidth={1.75} />
        </div>
        <p className={styles.eyebrow}>project-consoles</p>
        <h1 id="welcome-title" className={styles.title}>
          Seja bem-vindo ao project-consoles
        </h1>
        <p className={styles.subtitle}>
          Sua biblioteca de jogos e consoles, organizada em um só lugar.
        </p>
      </section>
    </main>
  );
}
