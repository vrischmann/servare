use uuid::Uuid;

pub struct UserID(Uuid);

pub struct User {
    pub id: UserID,
}

impl User {}
