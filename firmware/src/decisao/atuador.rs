//! Atuacao do rele (GPIO 26) - **Opcao A** (data-model §1):
//!   OFF -> rele ABERTO  -> GPIO LOW  (energia cortada)
//!   STD -> rele FECHADO -> GPIO HIGH (energizado, standby)
//!   ON  -> rele FECHADO -> GPIO HIGH (energizado, uso ativo)
//!
//! Regra: `rele_aberto = (estado == OFF)`.

use anyhow::Result;
use esp_idf_svc::hal::gpio::{Gpio26, Output, PinDriver};

use crate::solver::Estado;

/// Driver do rele. Mantem o pino de saida vivo enquanto existir.
/// (esp-idf-hal 0.46: `PinDriver<'d, MODE>` - sem parametro de pino; pinos tem lifetime.)
pub struct Atuador<'d> {
    pino: PinDriver<'d, Output>,
}

impl<'d> Atuador<'d> {
    /// Inicializa o pino do rele como saida. Comeca ABERTO (OFF) por seguranca.
    pub fn novo(pino: Gpio26<'d>) -> Result<Self> {
        let mut driver = PinDriver::output(pino)?;
        driver.set_low()?; // OFF = aberto
        Ok(Atuador { pino: driver })
    }

    /// Aplica o estado decidido: fecha o rele (HIGH) em STD/ON, abre (LOW) em OFF.
    pub fn aplicar(&mut self, estado: Estado) -> Result<()> {
        if rele_aberto(estado) {
            self.pino.set_low()?;
        } else {
            self.pino.set_high()?;
        }
        Ok(())
    }
}

/// `true` se o estado implica rele ABERTO (somente OFF).
pub fn rele_aberto(estado: Estado) -> bool {
    estado == Estado::Off
}
