//! Gancho pós-turno: política de domínio que o core não conhece.

use super::report::TurnReport;

// Roda depois de cada turno e devolve avisos para o usuário. O core não sabe o que é "arquivo" ou
// "remoção": quem sabe (ex.: `toolkit::claims`) se liga aqui. Um gancho só avisa, nunca bloqueia.
pub trait TurnHook {
    fn after_turn(&self, report: &TurnReport) -> Vec<String>;
}
