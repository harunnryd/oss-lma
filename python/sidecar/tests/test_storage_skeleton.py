def test_storage_package_is_importable():
    from sidecar import storage

    assert storage.__package__ == "sidecar.storage"
