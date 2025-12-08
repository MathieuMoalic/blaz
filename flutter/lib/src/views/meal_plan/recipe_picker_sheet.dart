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

  bool matches(Recipe r, String q) {
    if (q.isEmpty) return true;
    final needle = q.toLowerCase();
    if (r.title.toLowerCase().contains(needle)) return true;
    for (final ing in r.ingredients) {
      if (ing.name.toLowerCase().contains(needle)) return true;
      if (ing.toLine().toLowerCase().contains(needle)) return true;
    }
    return false;
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
          final filtered = all.where((r) => matches(r, query)).toList()
            ..sort(
              (a, b) => a.title.toLowerCase().compareTo(b.title.toLowerCase()),
            );

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
                                for (final r in filtered) selected.add(r.id);
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
                        hintText: 'Search recipes by name or ingredient',
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
                              final thumb = mediaUrl(r.imagePathSmall);
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
