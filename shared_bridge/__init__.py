"""Shared-Bridge V2: a minimal RackAI reservation manager.

The bridge owns no resources and performs no inference. It remembers one
selected logical service, asks RackAI for a reservation, and hands the
reservation-scoped access material to consumer applications.
"""
