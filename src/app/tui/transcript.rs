//! A conversa em tela cheia: as linhas guardadas, a rolagem e a seleção com o mouse.
//!
//! As linhas são guardadas como chegam e quebradas na largura atual da janela só na hora de desenhar
//! (com cache), então redimensionar a janela reflui o texto em vez de cortá-lo. A rolagem conta linhas
//! a partir do fim: 0 acompanha o que chega; com a conversa rolada para cima, o que chega não a move.

use super::print::wrap_line;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

// Máximo de linhas guardadas. As mais antigas saem primeiro.
const MAX_LINES: usize = 20_000;

// Uma posição na conversa: a linha já quebrada (contada do começo) e a coluna na tela.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Point {
    pub(super) row: usize,
    pub(super) col: usize,
}

// Trecho selecionado com o mouse: de onde o arrasto começou até onde está agora.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Selection {
    pub(super) anchor: Point,
    pub(super) head: Point,
}

impl Selection {
    // Início e fim em ordem de leitura. O fim é inclusive: a célula sob o mouse entra na seleção.
    fn ordered(&self) -> (Point, Point) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    // Colunas selecionadas na linha `row`, como intervalo [de, até).
    fn cols(&self, row: usize) -> Option<(usize, usize)> {
        let (start, end) = self.ordered();
        if row < start.row || row > end.row {
            return None;
        }
        let from = if row == start.row { start.col } else { 0 };
        let to = if row == end.row { end.col + 1 } else { usize::MAX };
        Some((from, to))
    }
}

#[derive(Default)]
pub(super) struct Transcript {
    lines: Vec<Line<'static>>,
    // Cache das linhas quebradas e a largura em que foram quebradas (0: precisa refazer).
    width: u16,
    rows: Vec<Line<'static>>,
    // Linhas roladas para cima a partir do fim.
    pub(super) scroll_back: usize,
    pub(super) selection: Option<Selection>,
    // A abertura já saiu do histórico (passou de `MAX_LINES`): não há mais cabeçalho para atualizar.
    head_lost: bool,
}

impl Transcript {
    pub(super) fn push(&mut self, lines: Vec<Line<'static>>) {
        if self.width > 0 {
            let width = usize::from(self.width);
            let before = self.rows.len();
            self.rows.extend(lines.iter().cloned().flat_map(|l| wrap_line(l, width)));
            // Rolado para cima: quem está lendo não é arrastado pelo que chega.
            if self.scroll_back > 0 {
                self.scroll_back += self.rows.len() - before;
            }
        }
        self.lines.extend(lines);
        if self.lines.len() > MAX_LINES {
            let extra = self.lines.len() - MAX_LINES;
            self.lines.drain(..extra);
            self.head_lost = true;
            self.width = 0;
            self.selection = None;
        }
    }

    // Troca as primeiras linhas (a abertura, enquanto o mascote anima) sem mexer no resto.
    pub(super) fn replace_head(&mut self, head: Vec<Line<'static>>) {
        if self.head_lost {
            return;
        }
        let n = head.len().min(self.lines.len());
        if self.lines[..n] == head[..n] {
            return;
        }
        self.lines.splice(..n, head.into_iter().take(n));
        self.width = 0;
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    // Quebra as linhas na largura dada, se ela mudou desde a última vez.
    pub(super) fn layout(&mut self, width: u16) {
        if width == self.width {
            return;
        }
        self.rows = self.lines.iter().cloned().flat_map(|l| wrap_line(l, usize::from(width))).collect();
        self.width = width;
        self.selection = None;
    }

    pub(super) fn total_rows(&self) -> usize {
        self.rows.len()
    }

    // Primeira linha visível numa área de `height` linhas, com a rolagem atual.
    pub(super) fn first_visible(&self, height: usize) -> usize {
        let end = self.rows.len().saturating_sub(self.scroll_back.min(self.max_back(height)));
        end.saturating_sub(height)
    }

    fn max_back(&self, height: usize) -> usize {
        self.rows.len().saturating_sub(height)
    }

    // Limita a rolagem ao que existe, para a roda não "acumular" além do começo.
    pub(super) fn clamp(&mut self, height: usize) {
        self.scroll_back = self.scroll_back.min(self.max_back(height));
    }

    // Rola `delta` linhas: positivo sobe (para o começo), negativo desce (para o fim).
    pub(super) fn scroll(&mut self, delta: isize, height: usize) {
        let back = self.scroll_back as isize + delta;
        self.scroll_back = (back.max(0) as usize).min(self.max_back(height));
    }

    pub(super) fn to_end(&mut self) {
        self.scroll_back = 0;
    }

    // As linhas que cabem em `height`, com a seleção em destaque. Antes, `layout` na mesma largura.
    pub(super) fn visible(&self, height: usize) -> Vec<Line<'static>> {
        let from = self.first_visible(height);
        let to = (from + height).min(self.rows.len());
        (from..to)
            .map(|row| match self.selection.and_then(|s| s.cols(row)) {
                Some((a, b)) => highlight(&self.rows[row], a, b),
                None => self.rows[row].clone(),
            })
            .collect()
    }

    // O texto selecionado, uma linha da tela por linha, sem os espaços que sobram no fim.
    pub(super) fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        let (start, end) = sel.ordered();
        if start == end {
            return None;
        }
        let rows: Vec<String> = (start.row..=end.row.min(self.rows.len().saturating_sub(1)))
            .map(|row| {
                let (a, b) = sel.cols(row).unwrap_or((0, 0));
                cells(&self.rows[row]).into_iter().filter(|(col, _, _)| *col >= a && *col < b).map(|(_, c, _)| c).collect::<String>().trim_end().to_string()
            })
            .collect();
        let text = rows.join("\n").trim_end().to_string();
        (!text.trim().is_empty()).then_some(text)
    }
}

// Cada caractere de uma linha com a coluna em que começa e o estilo.
fn cells(line: &Line<'static>) -> Vec<(usize, char, Style)> {
    let mut col = 0;
    let mut out = Vec::new();
    for span in &line.spans {
        for c in span.content.chars() {
            out.push((col, c, span.style));
            col += Span::raw(c.to_string()).width();
        }
    }
    out
}

// A mesma linha com as colunas [de, até) em vídeo reverso. Uma linha vazia selecionada ganha um espaço
// marcado, para a seleção de várias linhas não ter buracos.
fn highlight(line: &Line<'static>, from: usize, to: usize) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let all = cells(line);
    for (col, c, style) in &all {
        let style = if *col >= from && *col < to { style.add_modifier(Modifier::REVERSED) } else { *style };
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push(*c),
            _ => spans.push(Span::styled(c.to_string(), style)),
        }
    }
    if all.is_empty() && from == 0 && to > 0 {
        spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn filled(n: usize) -> Transcript {
        let mut t = Transcript::default();
        t.layout(40);
        t.push((0..n).map(|i| Line::from(format!("linha {i}"))).collect());
        t
    }

    #[test]
    fn it_follows_the_end_and_shows_the_last_rows() {
        let t = filled(10);
        let shown: Vec<String> = t.visible(3).iter().map(text).collect();
        assert_eq!(shown, ["linha 7", "linha 8", "linha 9"]);
        // Menos linhas que a altura: tudo, a partir do topo.
        assert_eq!(filled(2).visible(5).len(), 2);
    }

    #[test]
    fn scrolling_stops_at_the_top_and_new_lines_do_not_move_a_scrolled_view() {
        let mut t = filled(10);
        t.scroll(100, 3);
        assert_eq!(t.scroll_back, 7, "não passa do começo");
        assert_eq!(text(&t.visible(3)[0]), "linha 0");
        t.push(vec![Line::from("nova")]);
        assert_eq!(text(&t.visible(3)[0]), "linha 0", "quem lê lá em cima não é arrastado");
        t.to_end();
        assert_eq!(text(t.visible(3).last().unwrap()), "nova");
        t.scroll(-5, 3);
        assert_eq!(t.scroll_back, 0);
    }

    #[test]
    fn long_lines_reflow_when_the_width_changes() {
        let mut t = Transcript::default();
        t.push(vec![Line::from("uma frase comprida que precisa quebrar")]);
        t.layout(20);
        assert_eq!(t.total_rows(), 2);
        t.layout(80);
        assert_eq!(t.total_rows(), 1);
    }

    #[test]
    fn the_selection_is_highlighted_and_copied_as_text() {
        let mut t = filled(3);
        t.selection = Some(Selection { anchor: Point { row: 1, col: 6 }, head: Point { row: 0, col: 2 } });
        assert_eq!(t.selected_text().as_deref(), Some("nha 0\nlinha 1"));
        let rows = t.visible(3);
        let reversed = |l: &Line, s: &str| l.spans.iter().any(|sp| sp.content == s && sp.style.add_modifier.contains(Modifier::REVERSED));
        assert!(reversed(&rows[0], "nha 0"), "{:?}", rows[0]);
        assert!(reversed(&rows[1], "linha 1"), "{:?}", rows[1]);
        assert!(rows[2].spans.iter().all(|sp| !sp.style.add_modifier.contains(Modifier::REVERSED)));
        // Um clique sem arrastar não seleciona nada.
        t.selection = Some(Selection { anchor: Point { row: 1, col: 1 }, head: Point { row: 1, col: 1 } });
        assert_eq!(t.selected_text(), None);
    }

    #[test]
    fn the_head_can_be_replaced_while_the_mascot_animates() {
        let mut t = filled(3);
        t.replace_head(vec![Line::from("cabeçalho")]);
        t.layout(40);
        assert_eq!(text(&t.visible(3)[0]), "cabeçalho");
        assert_eq!(text(&t.visible(3)[1]), "linha 1");
    }

    #[test]
    fn a_head_that_left_the_history_is_not_replaced() {
        let mut t = filled(MAX_LINES + 5);
        t.replace_head(vec![Line::from("cabeçalho")]);
        t.layout(40);
        assert!(t.visible(3).iter().all(|l| text(l) != "cabeçalho"), "linhas de outro conteúdo não viram cabeçalho");
    }
}
