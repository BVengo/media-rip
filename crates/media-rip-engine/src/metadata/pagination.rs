/// A provider page, normalised at the HTTP boundary.
pub(super) struct Page<T> {
    pub items: Vec<T>,
    pub has_more: bool,
}

/// Never treat a failed or truncated search as a complete candidate set.
pub(super) fn collect_pages<T>(
    first: u32,
    limit: u32,
    mut fetch: impl FnMut(u32) -> Result<Page<T>, String>,
) -> Result<Vec<T>, String> {
    let mut items = Vec::new();
    for page_number in first..first.saturating_add(limit) {
        let page = fetch(page_number)?;
        if page.items.is_empty() && page.has_more {
            return Err("Provider returned an empty page with more results pending".to_owned());
        }
        items.extend(page.items);
        if !page.has_more {
            return Ok(items);
        }
    }
    Err("Metadata search exceeded the page limit; narrow the search before matching".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn later_candidates_are_included() {
        let items = collect_pages(1, 5, |page| {
            Ok(Page {
                items: vec![page],
                has_more: page < 3,
            })
        })
        .unwrap();
        assert_eq!(items, vec![1, 2, 3]);
    }
    #[test]
    fn failed_or_unbounded_pagination_never_returns_partial_candidates() {
        assert!(
            collect_pages(0, 2, |page| if page == 1 {
                Err("provider unavailable".into())
            } else {
                Ok(Page {
                    items: vec![page],
                    has_more: true,
                })
            })
            .is_err()
        );
        assert!(
            collect_pages(0, 2, |page| Ok(Page {
                items: vec![page],
                has_more: true
            }))
            .is_err()
        );
        assert!(
            collect_pages::<u32>(0, 2, |_| Ok(Page {
                items: vec![],
                has_more: true
            }))
            .is_err()
        );
    }
}
