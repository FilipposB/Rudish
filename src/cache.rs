use std::collections::HashMap;

pub struct Topic {
    data: HashMap<String, String>,
}

pub struct Cache {
    topics: HashMap<String, Topic>,
}

impl Topic {
    fn new() -> Topic {
        Topic {
            data: HashMap::new(),
        }
    }
}

impl Cache {
    pub fn new() -> Cache {
        Cache {
            topics: HashMap::new(),
        }
    }

    pub fn get(&self, topic_name: &str, key: &str) -> Result<String, String> {
        match self.topics.get(topic_name) {
            None => {Err(format!("Topic not found: {}", topic_name))},
            Some(topic) => {
                match topic.data.get(key) {
                    None => {Err(format!("Value not found: {}", topic_name))},
                    Some(value) => {Ok(value.to_string())}
                }
            }
        }
    }

    pub fn put(&mut self, topic: &str, key: &str, value: &str)  {
        self.topics
            .entry(topic.to_string())
            .or_insert(Topic::new())
            .data
            .entry(key.to_string())
            .or_insert(value.to_string());
    }

    pub fn delete(&mut self, topic: &str, key: &str) -> Result<(), String> {
        match self.topics.get_mut(topic) {
            Some(topic_data) => {
                topic_data.data.remove(key);
                Ok(())
            }
            None => Err(format!("Topic '{}' not found", topic)),
        }
    }
}
