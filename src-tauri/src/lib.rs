/// Inicia a aplicação Tauri e bloqueia até a janela principal ser fechada.
///
/// # Panics
///
/// Entra em pânico se o Tauri não conseguir inicializar o runtime ou a
/// janela (por exemplo, sem WebView disponível no sistema). Sem isso não
/// há interface para reportar o erro, então encerrar é o único caminho.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("falha ao iniciar a aplicação Tauri");
}
