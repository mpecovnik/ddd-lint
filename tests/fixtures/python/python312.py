"""Exercise target-version gating for the Python 3.12 type statement.

The adapter must reject this declaration when targeting Python 3.10 or 3.11
and accept it for Python 3.12 and newer.
"""

type Document = Draft | Published
