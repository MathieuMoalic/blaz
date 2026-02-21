import 'package:flutter/material.dart';
import '../../api.dart';

Future<Set<int>?> showRecipePickerSheet({
  required BuildContext context,
  required String dayIso,
  required List<Recipe> all,
}) async {
  final ctrl = TextEditingController();
  String query = '';
  final selected = <int>{};

  /// Character-by-character fuzzy matching
  /// Returns score 0-100 based on how many characters match in sequence
  double fuzzyCharMatch(String text, String pattern) {
    if (pattern.isEmpty) return 0;
    
    int patternIdx = 0;
    int lastMatchIdx = -1;
    int consecutiveMatches = 0;
    double score = 0;
    
    for (int i = 0; i < text.length && patternIdx < pattern.length; i++) {
      if (text[i] == pattern[patternIdx]) {
        // Bonus for consecutive matches
        if (i == lastMatchIdx + 1) {
          consecutiveMatches++;
          score += 10 + consecutiveMatches; // Increasing bonus
        } else {
          consecutiveMatches = 0;
          score += 5;
        }
        
        lastMatchIdx = i;
        patternIdx++;
      }
    }
    
    // All characters must match
    if (patternIdx != pattern.length) return 0;
    
    // Normalize score to 0-100
    return (score / pattern.length).clamp(0, 100);
  }

  /// Fuzzy matching with scoring for better results ordering
  double fuzzyScore(Recipe r, String q) {
    final needle = q.toLowerCase();
    final titleLower = r.title.toLowerCase();
    
    // Exact match (highest score)
    if (titleLower == needle) return 1000.0;
    
    // Title starts with query
    if (titleLower.startsWith(needle)) return 900.0;
    
    // Word boundary match (e.g., "chick" matches "Chicken Curry")
    final words = titleLower.split(RegExp(r'\s+'));
    for (final word in words) {
      if (word.startsWith(needle)) return 800.0;
    }
    
    // Contains exact substring
    if (titleLower.contains(needle)) return 700.0;
    
    // Fuzzy character sequence match
    double charScore = fuzzyCharMatch(titleLower, needle);
    if (charScore > 0) return 500.0 + charScore;
    
    // Search in ingredients
    for (final ing in r.ingredients) {
      final ingLower = ing.name.toLowerCase();
      if (ingLower.contains(needle)) return 400.0;
      
      final ingCharScore = fuzzyCharMatch(ingLower, needle);
      if (ingCharScore > 0) return 300.0 + ingCharScore;
    }
    
    return 0.0;
  }

  bool matches(Recipe r, String q) {
    if (q.isEmpty) return true;
    return fuzzyScore(r, q) > 0;
  }

  return showModalBottomSheet<Set<int>>(
    context: context,
    isScrollControlled: true,
    showDragHandle: true,
    builder: (ctx) {
      final media = MediaQuery.of(ctx);
      final height = media.size.height * 0.8;

      return StatefulBuilder(
        builder: (ctx, setSheetState) {
          final filtered = all.where((r) => matches(r, query)).toList();
          
          // Sort by fuzzy match score when searching, otherwise alphabetically
          if (query.isNotEmpty) {
            filtered.sort((a, b) {
              final scoreA = fuzzyScore(a, query);
              final scoreB = fuzzyScore(b, query);
              return scoreB.compareTo(scoreA); // Best matches first
            });
          } else {
            filtered.sort(
              (a, b) => a.title.toLowerCase().compareTo(b.title.toLowerCase()),
            );
          }

          void toggle(int id) {
            setSheetState(() {
              if (!selected.add(id)) selected.remove(id);
            });
          }

          final allSelected =
              filtered.isNotEmpty &&
              filtered.every((r) => selected.contains(r.id));
          final anySelected = filtered.any((r) => selected.contains(r.id));
          final triValue = allSelected
              ? true
              : anySelected
              ? null
              : false;

          return SafeArea(
            child: SizedBox(
              height: height,
              child: Column(
                children: [
                  // Header
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 10, 16, 6),
                    child: Row(
                      children: [
                        Expanded(
                          child: Text(
                            'Assign to $dayIso',
                            style: Theme.of(ctx).textTheme.titleMedium,
                          ),
                        ),
                        const SizedBox(width: 8),
                        const Text('Select all'),
                        Checkbox(
                          value: triValue,
                          tristate: true,
                          onChanged: (_) {
                            setSheetState(() {
                              selected.clear();
                              if (!allSelected) {
                                for (final r in filtered) {
                                  selected.add(r.id);
                                }
                              }
                            });
                          },
                        ),
                      ],
                    ),
                  ),
                  // Search
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                    child: TextField(
                      controller: ctrl,
                      textInputAction: TextInputAction.search,
                      decoration: InputDecoration(
                        hintText: 'Search recipes...',
                        prefixIcon: const Icon(Icons.search),
                        isDense: true,
                        border: OutlineInputBorder(
                          borderRadius: BorderRadius.circular(12),
                        ),
                        suffixIcon: query.isEmpty
                            ? null
                            : IconButton(
                                tooltip: 'Clear',
                                icon: const Icon(Icons.close),
                                onPressed: () => setSheetState(() {
                                  ctrl.clear();
                                  query = '';
                                }),
                              ),
                      ),
                      onChanged: (s) => setSheetState(() => query = s.trim()),
                    ),
                  ),
                  const Divider(height: 1),
                  // Results
                  Expanded(
                    child: filtered.isEmpty
                        ? const Center(child: Text('No matching recipes'))
                        : ListView.separated(
                            padding: const EdgeInsets.symmetric(vertical: 6),
                            itemCount: filtered.length,
                            separatorBuilder: (_, __) =>
                                const Divider(height: 1),
                            itemBuilder: (_, i) {
                              final r = filtered[i];
                              final thumb = mediaUrl(r.imagePathSmall, cacheBuster: r.updatedAt);
                              final checked = selected.contains(r.id);
                              return ListTile(
                                onTap: () => toggle(r.id),
                                leading: thumb == null
                                    ? const SizedBox(
                                        width: 44,
                                        height: 44,
                                        child: Icon(Icons.image_not_supported),
                                      )
                                    : ClipRRect(
                                        borderRadius: BorderRadius.circular(6),
                                        child: Image.network(
                                          thumb,
                                          width: 44,
                                          height: 44,
                                          fit: BoxFit.cover,
                                        ),
                                      ),
                                title: Text(r.title),
                                trailing: Checkbox(
                                  value: checked,
                                  onChanged: (_) => toggle(r.id),
                                ),
                              );
                            },
                          ),
                  ),
                  const Divider(height: 1),
                  // Actions
                  Padding(
                    padding: EdgeInsets.only(
                      left: 16,
                      right: 16,
                      top: 10,
                      bottom: MediaQuery.of(ctx).viewInsets.bottom + 12,
                    ),
                    child: Row(
                      children: [
                        Expanded(
                          child: OutlinedButton(
                            onPressed: () => Navigator.pop(ctx),
                            child: const Text('Cancel'),
                          ),
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: FilledButton.icon(
                            onPressed: selected.isEmpty
                                ? null
                                : () => Navigator.pop(
                                    ctx,
                                    Set<int>.from(selected),
                                  ),
                            icon: const Icon(Icons.event_available),
                            label: Text(
                              selected.isEmpty
                                  ? 'Assign'
                                  : 'Assign ${selected.length}',
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          );
        },
      );
    },
  );
}
