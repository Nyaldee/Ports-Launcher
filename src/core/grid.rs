
pub const CARD_WIDTH: f32 = 200.0;
pub const CARD_HEIGHT: f32 = 300.0;
pub const CARD_SPACING: f32 = 14.0;

pub fn compute_grid_columns(available_width: f32) -> usize {
    let col_width = CARD_WIDTH + CARD_SPACING;
    if available_width < col_width {
        return 1;
    }
    (available_width / col_width).floor() as usize
}

pub fn chunk_into_rows<T: Clone>(items: &[T], columns: usize) -> Vec<Vec<T>> {
    let columns = columns.max(1);
    items.chunks(columns).map(<[T]>::to_vec).collect()
}

pub fn next_grid_position(current: (usize, usize), dx: i32, dy: i32, columns: usize, len: usize) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let columns = columns.max(1);
    let row_count = len.div_ceil(columns);
    let (row, col) = current;
    let flat = (row * columns + col) as i32;
    let next_flat = (flat + dx + dy * columns as i32).clamp(0, len as i32 - 1) as usize;
    let next = (next_flat / columns, next_flat % columns);
    if next.0 >= row_count {
        return None;
    }
    Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colonnes_arrondies_vers_le_bas() {
        assert_eq!(compute_grid_columns(988.0), 4);
    }

    #[test]
    fn colonnes_jamais_zero() {
        assert_eq!(compute_grid_columns(0.0), 1);
        assert_eq!(compute_grid_columns(-50.0), 1);
    }

    #[test]
    fn chunk_groupe_par_lignes_avec_reste() {
        let items = vec![1, 2, 3, 4, 5];
        let rows = chunk_into_rows(&items, 2);
        assert_eq!(rows, vec![vec![1, 2], vec![3, 4], vec![5]]);
    }

    #[test]
    fn chunk_colonnes_zero_traite_comme_une_seule_colonne() {
        let items = vec![1, 2];
        let rows = chunk_into_rows(&items, 0);
        assert_eq!(rows, vec![vec![1], vec![2]]);
    }

    #[test]
    fn next_grid_position_reste_toujours_dans_les_bornes_reelles() {
        for len in 0..=37usize {
            for columns in 1..=8usize {
                let row_count = if len == 0 { 0 } else { len.div_ceil(columns) };
                for start_flat in 0..len.max(1) {
                    let start = (start_flat / columns, start_flat % columns);
                    for dx in [-5, -1, 0, 1, 5] {
                        for dy in [-5, -1, 0, 1, 5] {
                            if let Some((row, col)) = next_grid_position(start, dx, dy, columns, len) {
                                let flat = row * columns + col;
                                assert!(flat < len, "len={len} columns={columns} start={start:?} dx={dx} dy={dy}: flat={flat} hors bornes (len={len})");
                                assert!(
                                    row < row_count,
                                    "len={len} columns={columns} start={start:?} dx={dx} dy={dy}: row={row} >= row_count={row_count}"
                                );
                            } else {
                                assert_eq!(len, 0, "len={len} columns={columns} start={start:?} dx={dx} dy={dy}: None alors que len > 0");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn next_grid_position_catalogue_vide_ne_renvoie_jamais_de_position() {
        for columns in 1..=8usize {
            assert_eq!(next_grid_position((0, 0), 1, 1, columns, 0), None);
            assert_eq!(next_grid_position((0, 0), 0, 0, columns, 0), None);
        }
    }

    #[test]
    fn next_grid_position_carte_seule_sur_derniere_ligne_incomplete() {
        let len = 7;
        let columns = 4;
        assert_eq!(next_grid_position((1, 2), 1, 0, columns, len), Some((1, 2)));
        assert_eq!(next_grid_position((1, 2), 5, 0, columns, len), Some((1, 2)));
        assert_eq!(next_grid_position((0, 0), 0, 1, columns, len), Some((1, 0)));
    }
}
