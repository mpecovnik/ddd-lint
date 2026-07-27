# E2E Fixture Coverage

| Rule | Positive | Negative | Suppression | Uncertainty |
| --- | --- | --- | --- | --- |
| DM001 | `flat/models.py:NullableOrder` | `flat/clean.py:RequiredOrder` | `flat/models.py:SuppressedOrder.optional_note` | `flat/uncertain.py:DynamicRecord` |
| DM002 | `flat/models.py:SparseOrder` | `flat/clean.py:SmallRecord` | `flat/models.py:SuppressedOrder` | `flat/uncertain.py:DynamicRecord` |
| DM003 | `flat/models.py:DefaultedOrder` | `flat/clean.py:RequiredOrder` | `flat/models.py:SuppressedOrder.source` | `flat/uncertain.py:DynamicRecord` |
| CT001 | `flat/handlers.py:incomplete` | `flat/clean.py:exhaustive` | `flat/suppressed.py:suppressed_incomplete` | `flat/uncertain.py:overlap` |
| CT002 | `flat/handlers.py:catch_all` | `flat/clean.py:exhaustive` | `flat/suppressed.py:suppressed_catch_all` | `flat/uncertain.py:overlap` |
| CT003 | `flat/handlers.py:conditional` | `flat/clean.py:exhaustive` | `flat/suppressed.py:suppressed_conditional` | `flat/uncertain.py:helper_dispatch` |
| CT004 | `flat/handlers.py:by_discriminator` | `flat/clean.py:exhaustive` | `flat/suppressed.py:suppressed_discriminator` | `flat/uncertain.py:ambiguous_discriminator` |
| DI001 | `flat/services.py:Service` | `flat/clean.py:InjectedService` | `flat/services.py:PreviewService` | `flat/uncertain.py:DynamicService` |
| SUP001 | `flat/suppressed.py:stale_exception` | used exceptions throughout `flat` | `malformed-sources/invalid_directive.py` rejects self-suppression | `flat/uncertain.py:dynamic_exception` |

`SUP001` cannot suppress itself by contract. Its suppression fixture therefore proves the directive is rejected rather than silently accepted.
